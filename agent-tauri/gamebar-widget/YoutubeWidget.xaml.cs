using System;
using System.Linq;
using System.Text.RegularExpressions;
using System.Threading.Tasks;
using Microsoft.Gaming.XboxGameBar;
using Windows.Storage;
using Windows.System;
using Windows.UI.Core;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;
using Windows.UI.Xaml.Navigation;
namespace YummiGameBarWidget
{
    public sealed partial class YoutubeWidget : Page
    {
        private XboxGameBarWidget widget;
        private Microsoft.UI.Xaml.Controls.WebView2 webView2;
        private bool webView2Unavailable, usingFallback, ready, playing;
        private string loadedVideoId;
        private long lastActionRequest;
        private double volume = 60;
        public YoutubeWidget() { InitializeComponent(); Loaded += OnLoaded; Unloaded += OnUnloaded; }
        protected override void OnNavigatedTo(NavigationEventArgs e) { widget = e.Parameter as XboxGameBarWidget; base.OnNavigatedTo(e); }
        private void OnLoaded(object sender, RoutedEventArgs e) { ApplicationData.Current.DataChanged += PreferencesChanged; Window.Current.CoreWindow.KeyDown += CoreWindow_KeyDown; ApplyPreferences(true); App.LogStage("youtube-widget-ready"); }
        private void OnUnloaded(object sender, RoutedEventArgs e) { ApplicationData.Current.DataChanged -= PreferencesChanged; Window.Current.CoreWindow.KeyDown -= CoreWindow_KeyDown; }
        private async void PreferencesChanged(ApplicationData sender, object args) { try { await Dispatcher.RunAsync(CoreDispatcherPriority.Normal, () => ApplyPreferences(false)); } catch { } }
        private void ApplyPreferences(bool forceLoad)
        {
            volume = YoutubePreferences.Volume;
            var videoId = ExtractYoutubeVideoId(YoutubePreferences.Video);
            if (!string.IsNullOrWhiteSpace(videoId) && (forceLoad || !string.Equals(videoId, loadedVideoId, StringComparison.Ordinal))) { loadedVideoId = videoId; _ = LoadYoutubeAsync(videoId); }
            var request = YoutubePreferences.ActionRequest;
            if (request != lastActionRequest) { lastActionRequest = request; if (ready) { var action = YoutubePreferences.Action; if (action == "play") { playing = true; _ = InvokeAsync("yummiPlay"); } else if (action == "pause") { playing = false; _ = InvokeAsync("yummiPause"); } } }
            if (ready) _ = SetVolumeAsync(volume);
        }
        private async void CoreWindow_KeyDown(CoreWindow sender, KeyEventArgs args)
        {
            if (args.KeyStatus.WasKeyDown || args.VirtualKey != VirtualKey.Space || !ready) return;
            args.Handled = true;
            if (playing) { if (await InvokeAsync("yummiPause")) playing = false; } else { if (await InvokeAsync("yummiPlay")) playing = true; }
        }
        private async Task LoadYoutubeAsync(string id)
        {
            ready = false; playing = false;
            var url = "https://yummi.duckdns.org/gamebar/youtube-player.html?v=" + Uri.EscapeDataString(id) + "&volume=" + ((int)Math.Round(volume));
            if (await EnsureWebView2Async()) { usingFallback = false; YoutubeFallbackWebView.Visibility = Visibility.Collapsed; webView2.Visibility = Visibility.Visible; webView2.Source = new Uri(url); }
            else { usingFallback = true; YoutubeFallbackWebView.Visibility = Visibility.Visible; YoutubeFallbackWebView.Navigate(new Uri(url)); }
        }
        private async Task<bool> EnsureWebView2Async()
        {
            if (webView2Unavailable) return false;
            if (webView2 != null && webView2.CoreWebView2 != null) return true;
            try
            {
                if (webView2 == null) { EnsureWinUiResources(); webView2 = new Microsoft.UI.Xaml.Controls.WebView2 { HorizontalAlignment = HorizontalAlignment.Stretch, VerticalAlignment = VerticalAlignment.Stretch }; webView2.NavigationCompleted += WebView2_NavigationCompleted; YoutubeWebHost.Children.Insert(0, webView2); }
                await webView2.EnsureCoreWebView2Async(); if (webView2.CoreWebView2 == null) throw new InvalidOperationException("CoreWebView2 unavailable");
                webView2.CoreWebView2.Settings.AreDefaultContextMenusEnabled = false; webView2.CoreWebView2.Settings.AreDevToolsEnabled = false; webView2.CoreWebView2.Settings.IsStatusBarEnabled = false; return true;
            }
            catch (Exception ex) { webView2Unavailable = true; usingFallback = true; if (webView2 != null) webView2.Visibility = Visibility.Collapsed; App.LogStage("youtube-only-webview2-fallback-" + ex.HResult.ToString("X8")); return false; }
        }
        private static void EnsureWinUiResources() { var resources = Application.Current.Resources; foreach (var d in resources.MergedDictionaries) if (d is Microsoft.UI.Xaml.Controls.XamlControlsResources) return; resources.MergedDictionaries.Add(new Microsoft.UI.Xaml.Controls.XamlControlsResources()); }
        private void WebView2_NavigationCompleted(Microsoft.UI.Xaml.Controls.WebView2 sender, Microsoft.Web.WebView2.Core.CoreWebView2NavigationCompletedEventArgs args) { ready = args.IsSuccess; if (ready) _ = SetVolumeAsync(volume); }
        private void YoutubeFallbackWebView_NavigationCompleted(WebView sender, WebViewNavigationCompletedEventArgs args) { ready = args.IsSuccess; if (ready) _ = SetVolumeAsync(volume); }
        private async Task SetVolumeAsync(double value) { if (!ready) return; try { var raw = ((int)Math.Round(Math.Max(0, Math.Min(100, value)))).ToString(); if (!usingFallback && webView2 != null && webView2.CoreWebView2 != null) await webView2.ExecuteScriptAsync("window.yummiSetVolume && window.yummiSetVolume(" + raw + ");"); else await YoutubeFallbackWebView.InvokeScriptAsync("yummiSetVolume", new[] { raw }); } catch { } }
        private async Task<bool> InvokeAsync(string fn) { if (!ready) return false; try { if (!usingFallback && webView2 != null && webView2.CoreWebView2 != null) await webView2.ExecuteScriptAsync("window." + fn + " && window." + fn + "();"); else await YoutubeFallbackWebView.InvokeScriptAsync(fn, new string[0]); return true; } catch { return false; } }
        private static string ExtractYoutubeVideoId(string input)
        {
            var raw = (input ?? "").Trim(); if (Regex.IsMatch(raw, "^[A-Za-z0-9_-]{11}$")) return raw;
            Uri uri; if (!Uri.TryCreate(raw, UriKind.Absolute, out uri) && !Uri.TryCreate("https://" + raw, UriKind.Absolute, out uri)) return null;
            var host = uri.Host.ToLowerInvariant(); string candidate = null;
            if (host == "youtu.be" || host.EndsWith(".youtu.be")) candidate = uri.AbsolutePath.Trim('/').Split('/').FirstOrDefault();
            else if (host == "youtube.com" || host.EndsWith(".youtube.com") || host == "youtube-nocookie.com" || host.EndsWith(".youtube-nocookie.com")) { var path = uri.AbsolutePath.Trim('/').Split(new[] {'/'}, StringSplitOptions.RemoveEmptyEntries); if (path.Length >= 2 && (path[0] == "embed" || path[0] == "shorts" || path[0] == "live")) candidate = path[1]; if (candidate == null) { var m = Regex.Match(uri.Query ?? "", "(?:^|[?&])v=([A-Za-z0-9_-]{11})(?:&|$)"); if (m.Success) candidate = m.Groups[1].Value; } }
            return candidate != null && Regex.IsMatch(candidate, "^[A-Za-z0-9_-]{11}$") ? candidate : null;
        }
    }
}
