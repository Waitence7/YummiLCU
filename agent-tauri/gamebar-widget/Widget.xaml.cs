using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Linq;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Gaming.XboxGameBar;
using Windows.Data.Json;
using Windows.ApplicationModel;
using Windows.Storage;
using Windows.Graphics.Display;
using Windows.Networking;
using Windows.Networking.Sockets;
using Windows.UI;
using Windows.UI.Core;
using Windows.System;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;
using Windows.UI.Xaml.Controls.Primitives;
using Windows.UI.Xaml.Media;
using Windows.UI.Xaml.Input;
using Windows.UI.Xaml.Navigation;

namespace YummiGameBarWidget
{
    public sealed partial class Widget : Page
    {
        private readonly DispatcherTimer timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(1) };
        private XboxGameBarWidget widget;
        private bool refreshing;
        private double displayWidth;
        private double displayHeight;
        private double displayScale = 1;
        private bool fittingDisplay;
        private bool resizeAccepted;
        private double fittedWidth;
        private double fittedHeight;
        private double fitTargetWidth;
        private double fitTargetHeight;
        private readonly SemaphoreSlim statusWrite = new SemaphoreSlim(1, 1);
        private string lastSavedStatus;
        private string bridgeStatus = "checking";
        private string bridgeError = "";
        private long lastFitRequest;
        private bool youtubeReady;
        private bool youtubeCollapsed;
        private bool youtubePlaying;
        private double youtubeVolume = 60;
        private readonly TranslateTransform hudPanelTransform = new TranslateTransform();
        private uint? hudDragPointerId;
        private Windows.Foundation.Point hudDragStartPoint;
        private double hudDragStartX;
        private double hudDragStartY;
        private const string HudPanelXSetting = "hud-panel-x";
        private const string HudPanelYSetting = "hud-panel-y";

        public Widget()
        {
            InitializeComponent();
            YoutubeWebView.AllowedScriptNotifyUris = new[] { new Uri("https://yummi.duckdns.org") };
            HudPanel.RenderTransform = hudPanelTransform;
            RestoreHudPanelPosition();
            SizeChanged += (_, __) => ClampHudPanelPosition();
            timer.Tick += async (_, __) => await RefreshAsync();
            Loaded += async (_, __) =>
            {
                App.LogStage("hud-loaded");
                Window.Current.CoreWindow.KeyDown += CoreWindowKeyDown;
                ApplicationData.Current.DataChanged += PreferencesChanged;
                lastFitRequest = HudPreferences.FitRequest;
                ApplyPreferences();
                // Start large, then adapt once to this display. Never overwrite
                // a pinned, explicitly configured or previously initialized layout.
                if (!HudPreferences.InitialFitDone)
                {
                    bool adapt = HudPreferences.FitRequest == 0 && !widget.Pinned;
                    HudPreferences.MarkInitialFitDone();
                    if (adapt) await FitToDisplayAsync();
                }
                UpdateInputState();
                timer.Start();
                await RefreshAsync();
                App.LogStage("hud-ready");
            };
            Unloaded += (_, __) =>
            {
                Window.Current.CoreWindow.KeyDown -= CoreWindowKeyDown;
                ApplicationData.Current.DataChanged -= PreferencesChanged;
                timer.Stop();
            };
        }

        private void CoreWindowKeyDown(CoreWindow sender, KeyEventArgs args)
        {
            if (args.KeyStatus.WasKeyDown) return;

            if (args.VirtualKey == VirtualKey.F8)
            {
                // CoreWindow raises this on the view dispatcher. Handle the event
                // before returning; its WinRT args must not be used after an await.
                args.Handled = true;
                HudPreferences.CoverageTint = !HudPreferences.CoverageTint;
                ApplyPreferences();
                return;
            }

            if (args.VirtualKey == VirtualKey.Space && youtubeReady && YoutubeBody.Visibility == Visibility.Visible)
            {
                var focused = FocusManager.GetFocusedElement() as DependencyObject;
                if (focused is TextBox || focused is WebView) return;
                args.Handled = true;
                _ = ToggleYoutubePlaybackAsync();
            }
        }

        private void ApplyPreferences()
        {
            HudPanel.Visibility = HudPreferences.Visible ? Visibility.Visible : Visibility.Collapsed;
            HudPanel.Opacity = HudPreferences.Opacity;
            RootGrid.Background = HudPreferences.CoverageTint
                ? new SolidColorBrush(Color.FromArgb(26, 123, 90, 166))
                : new SolidColorBrush(Colors.Transparent);
        }

        private async void PreferencesChanged(ApplicationData sender, object args)
        {
            try
            {
                await Dispatcher.RunAsync(CoreDispatcherPriority.Normal, () =>
                {
                    try
                    {
                        ApplyPreferences();
                        var request = HudPreferences.FitRequest;
                        if (request != lastFitRequest)
                        {
                            lastFitRequest = request;
                            _ = FitToDisplayAsync(HudPreferences.CompactLayout);
                        }
                    }
                    catch { }
                });
            }
            catch { }
        }

        protected override void OnNavigatedTo(NavigationEventArgs e)
        {
            widget = e.Parameter as XboxGameBarWidget;
            if (widget != null)
            {
                widget.SettingsClicked += WidgetSettingsClicked;
                widget.WindowBoundsChanged += WidgetStateChanged;
                widget.PinnedChanged += WidgetStateChanged;
                widget.ClickThroughEnabledChanged += WidgetStateChanged;
                widget.GameBarDisplayModeChanged += DisplayModeChanged;
            }
            base.OnNavigatedTo(e);
        }

        protected override void OnNavigatedFrom(NavigationEventArgs e)
        {
            if (widget != null)
            {
                widget.SettingsClicked -= WidgetSettingsClicked;
                widget.WindowBoundsChanged -= WidgetStateChanged;
                widget.PinnedChanged -= WidgetStateChanged;
                widget.ClickThroughEnabledChanged -= WidgetStateChanged;
                widget.GameBarDisplayModeChanged -= DisplayModeChanged;
            }
            base.OnNavigatedFrom(e);
        }

        private async void WidgetSettingsClicked(XboxGameBarWidget sender, object args)
        {
            try
            {
                await Dispatcher.RunAsync(CoreDispatcherPriority.Normal, async () =>
                {
                    try { await widget.ActivateSettingsAsync(); }
                    catch { InputText.Text = "설정을 열지 못했습니다. Game Bar에서 다시 열어 주세요."; }
                });
            }
            catch { }
        }

        private async void WidgetStateChanged(XboxGameBarWidget sender, object args)
        {
            await Dispatcher.RunAsync(CoreDispatcherPriority.Normal, () =>
            {
                UpdateDisplayState();
                UpdateInputState();
            });
        }

        private async void DisplayModeChanged(XboxGameBarWidget sender, object args)
        {
            await Dispatcher.RunAsync(CoreDispatcherPriority.Normal, () =>
            {
                // Opening/closing Game Bar must not resize or recenter a layout
                // that the user positioned and pinned through the host UI.
                UpdateDisplayState();
                UpdateInputState();
            });
        }

        private async Task FitToDisplayAsync(bool centerForPinSetup = false)
        {
            if (widget == null || fittingDisplay) return;
            fittingDisplay = true;
            try
            {
                UpdateDisplayMetrics();
                var minimum = widget.MinWindowSize;
                var maximum = widget.MaxWindowSize;
                var host = widget.WindowBounds;
                // Display dimensions and Game Bar size requests are DIPs. A
                // provisional activation-view size is not a DPI conversion.
                var width = HudPreferences.CompactLayout ? minimum.Width : Math.Max(minimum.Width, Math.Min(maximum.Width, displayWidth));
                var height = HudPreferences.CompactLayout ? minimum.Height : Math.Max(minimum.Height, Math.Min(maximum.Height, displayHeight));
                // Keep the manifest's minimum size: Game Bar must be able to fit
                // the content around its chrome and home bar.
                fitTargetWidth = width;
                fitTargetHeight = height;
                var result = await HudGeometry.FitLargestAsync(width, height, minimum.Width, minimum.Height,
                    ResizeHudAsync);
                resizeAccepted = result.Accepted;
                fittedWidth = result.Width;
                fittedHeight = result.Height;
                // Compact mode deliberately brings the titlebar back on screen.
                // Expanding must preserve the position chosen by the user.
                if (centerForPinSetup) await widget.CenterWindowAsync();
                UpdateDisplayState();
            }
            catch (Exception ex)
            {
                DisplayText.Text = "화면 맞춤 실패 · " + ErrorCode(ex);
            }
            finally { fittingDisplay = false; }
        }

        private async Task<bool> ResizeHudAsync(double width, double height)
        {
            var accepted = await widget.TryResizeWindowAsync(new Windows.Foundation.Size(width, height));
            App.LogStage($"fit {widget.GameBarDisplayMode} {width}x{height} accepted={accepted}");
            return accepted;
        }

        private void UpdateDisplayMetrics()
        {
            var display = DisplayInformation.GetForCurrentView();
            displayScale = display.RawPixelsPerViewPixel;
            displayWidth = HudGeometry.ToLogicalPixels(display.ScreenWidthInRawPixels, displayScale);
            displayHeight = HudGeometry.ToLogicalPixels(display.ScreenHeightInRawPixels, displayScale);
        }

        private void UpdateDisplayState()
        {
            if (widget == null) return;
            UpdateDisplayMetrics();
            var bounds = widget.WindowBounds;
            if (bounds.Width <= 0 || bounds.Height <= 0) return;
            // Dimensions alone do not establish screen coverage or position.
            // Report the rendered XAML size and host position without claiming
            // fullscreen success from a matching pair of numbers.
            DisplayText.Text = "HUD 표시 " + Math.Round(ActualWidth) + "×" + Math.Round(ActualHeight) +
                " · 위치 " + Math.Round(bounds.X) + "," + Math.Round(bounds.Y);
        }

        private void UpdateInputState()
        {
            if (widget == null) return;
            bool pinnedOnly = widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly;
            bool passesClicks = pinnedOnly && widget.Pinned && widget.ClickThroughEnabled;
            IsHitTestVisible = !passesClicks;
            if (passesClicks) InputText.Text = "게임으로 클릭 통과 중";
            else if (!widget.Pinned) InputText.Text = "HUD를 고정하면 게임 위에 표시됩니다";
            else if (!widget.ClickThroughEnabled) InputText.Text = "Game Bar 상단에서 클릭 통과를 켜 주세요";
            else InputText.Text = "Game Bar를 닫으면 게임으로 클릭이 통과합니다";
            SaveStatus();
        }

        private async void SaveStatus()
        {
            try
            {
                if (widget == null) return;
                var bounds = widget.WindowBounds;
                var version = Package.Current.Id.Version;
                var status = new JsonObject
                {
                    ["processId"] = JsonValue.CreateNumberValue(Windows.System.Diagnostics.ProcessDiagnosticInfo.GetForCurrentProcess().ProcessId),
                    ["version"] = JsonValue.CreateStringValue(version.Major + "." + version.Minor + "." + version.Build + "." + version.Revision),
                    ["bridge"] = JsonValue.CreateStringValue(bridgeStatus),
                    ["compactLayout"] = JsonValue.CreateBooleanValue(HudPreferences.CompactLayout),
                    ["hudVisible"] = JsonValue.CreateBooleanValue(HudPreferences.Visible),
                    ["hudOpacity"] = JsonValue.CreateNumberValue(HudPreferences.Opacity),
                    ["coverageTint"] = JsonValue.CreateBooleanValue(HudPreferences.CoverageTint),
                    ["error"] = JsonValue.CreateStringValue(bridgeError),
                    ["pinned"] = JsonValue.CreateBooleanValue(widget.Pinned),
                    ["clickThroughEnabled"] = JsonValue.CreateBooleanValue(widget.ClickThroughEnabled),
                    ["displayMode"] = JsonValue.CreateStringValue(widget.GameBarDisplayMode.ToString()),
                    ["x"] = JsonValue.CreateNumberValue(bounds.X),
                    ["y"] = JsonValue.CreateNumberValue(bounds.Y),
                    ["contentWidth"] = JsonValue.CreateNumberValue(ActualWidth),
                    ["contentHeight"] = JsonValue.CreateNumberValue(ActualHeight),
                    ["displayScale"] = JsonValue.CreateNumberValue(displayScale),
                    ["resizeAccepted"] = JsonValue.CreateBooleanValue(resizeAccepted),
                    ["fitTargetWidth"] = JsonValue.CreateNumberValue(fitTargetWidth),
                    ["fitTargetHeight"] = JsonValue.CreateNumberValue(fitTargetHeight),
                    ["horizontalResizeSupported"] = JsonValue.CreateBooleanValue(widget.HorizontalResizeSupported),
                    ["verticalResizeSupported"] = JsonValue.CreateBooleanValue(widget.VerticalResizeSupported),
                    ["minWidth"] = JsonValue.CreateNumberValue(widget.MinWindowSize.Width),
                    ["minHeight"] = JsonValue.CreateNumberValue(widget.MinWindowSize.Height),
                    ["maxWidth"] = JsonValue.CreateNumberValue(widget.MaxWindowSize.Width),
                    ["maxHeight"] = JsonValue.CreateNumberValue(widget.MaxWindowSize.Height),
                    ["fittedWidth"] = JsonValue.CreateNumberValue(fittedWidth),
                    ["fittedHeight"] = JsonValue.CreateNumberValue(fittedHeight),
                    ["width"] = JsonValue.CreateNumberValue(bounds.Width),
                    ["height"] = JsonValue.CreateNumberValue(bounds.Height),
                    ["displayWidth"] = JsonValue.CreateNumberValue(displayWidth),
                    ["displayHeight"] = JsonValue.CreateNumberValue(displayHeight)
                }.Stringify();
                // One bounded local state file, updated only on changes. No LCU
                // payload, account name or credentials are written.
                await statusWrite.WaitAsync();
                try
                {
                    if (status == lastSavedStatus) return;
                    var file = await ApplicationData.Current.LocalFolder.CreateFileAsync("hud-status.json", CreationCollisionOption.ReplaceExisting);
                    await FileIO.WriteTextAsync(file, status);
                    lastSavedStatus = status;
                }
                catch { }
                finally { statusWrite.Release(); }
            }
            catch { }
        }

        private async Task RefreshAsync()
        {
            if (refreshing) return;
            refreshing = true;
            try
            {
                string line;
                try { line = await NativePipeClient.ReadSnapshotAsync(); }
                catch (Exception pipeError)
                {
                    try { line = await ReadLoopbackSnapshotAsync(); }
                    catch (Exception socketError)
                    {
                        throw new AggregateException(pipeError, socketError);
                    }
                }
                if (string.IsNullOrWhiteSpace(line)) throw new IOException("empty bridge response");
                Apply(JsonObject.Parse(line));
            }
            catch (Exception ex) { SetOffline(ex); }
            finally { refreshing = false; }
        }

        private async Task<string> ReadLoopbackSnapshotAsync()
        {
            using (var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(2)))
            using (var socket = new StreamSocket())
            {
                await socket.ConnectAsync(new HostName("127.0.0.1"), "47658").AsTask(timeout.Token);
                using (var stream = socket.InputStream.AsStreamForRead())
                    return await NativePipeClient.ReadLineAsync(stream, timeout.Token);
            }
        }

        private void Apply(JsonObject json)
        {
            bool connected = json.GetNamedBoolean("connected", false);
            string phase = SnapshotJson.String(json, "phase", connected ? "Unknown" : "Offline");
            string riotId = SnapshotJson.String(json, "riot_id");
            var party = new List<string>();
            if (json.ContainsKey("party_members") && json["party_members"].ValueType == JsonValueType.Array)
                foreach (var value in json.GetNamedArray("party_members")) if (value.ValueType == JsonValueType.String) party.Add(value.GetString());

            PhaseText.Text = phase;
            RiotIdText.Text = string.IsNullOrWhiteSpace(riotId) ? (connected ? "Riot ID 확인 중" : "League 클라이언트를 기다리는 중") : riotId;
            PartyText.Text = party.Count == 0 ? "파티 정보 없음" : string.Join(" · ", party.Take(5));
            bridgeStatus = "connected";
            bridgeError = "";
            BridgeText.Text = connected ? "LCU 연결됨" : "유미 에이전트 연결됨 · League 대기 중";
            StatusDot.Fill = new SolidColorBrush(connected ? Color.FromArgb(255, 86, 221, 139) : Color.FromArgb(255, 122, 106, 134));
            SaveStatus();
        }

        private void HudDragHandle_PointerPressed(object sender, PointerRoutedEventArgs e)
        {
            if (e.OriginalSource is Button || IsInteractiveHudElement(e.OriginalSource as DependencyObject)) return;
            var point = e.GetCurrentPoint(RootGrid);
            if (!point.Properties.IsLeftButtonPressed) return;

            hudDragPointerId = e.Pointer.PointerId;
            hudDragStartPoint = point.Position;
            hudDragStartX = hudPanelTransform.X;
            hudDragStartY = hudPanelTransform.Y;
            HudDragHandle.CapturePointer(e.Pointer);
            e.Handled = true;
        }

        private void HudDragHandle_PointerMoved(object sender, PointerRoutedEventArgs e)
        {
            if (hudDragPointerId != e.Pointer.PointerId) return;
            var point = e.GetCurrentPoint(RootGrid);
            if (!point.Properties.IsLeftButtonPressed)
            {
                EndHudPanelDrag(e);
                return;
            }

            var nextX = hudDragStartX + point.Position.X - hudDragStartPoint.X;
            var nextY = hudDragStartY + point.Position.Y - hudDragStartPoint.Y;
            SetHudPanelPosition(nextX, nextY, false);
            e.Handled = true;
        }

        private void HudDragHandle_PointerReleased(object sender, PointerRoutedEventArgs e)
        {
            if (hudDragPointerId != e.Pointer.PointerId) return;
            EndHudPanelDrag(e);
            SnapHudPanelToNearestCorner();
            SaveHudPanelPosition();
            e.Handled = true;
        }

        private void HudDragHandle_PointerCanceled(object sender, PointerRoutedEventArgs e)
        {
            if (hudDragPointerId != e.Pointer.PointerId) return;
            EndHudPanelDrag(e);
        }

        private void HudDragHandle_PointerCaptureLost(object sender, PointerRoutedEventArgs e)
        {
            if (hudDragPointerId == e.Pointer.PointerId) hudDragPointerId = null;
        }

        private void EndHudPanelDrag(PointerRoutedEventArgs e)
        {
            HudDragHandle.ReleasePointerCapture(e.Pointer);
            hudDragPointerId = null;
        }

        private static bool IsInteractiveHudElement(DependencyObject source)
        {
            for (var current = source; current != null; current = VisualTreeHelper.GetParent(current))
            {
                if (current is TextBox || current is Button || current is Slider || current is WebView)
                    return true;
                if (current is Border border && border.Name == "HudPanel") break;
            }
            return false;
        }


        private void HudResetPositionButton_Click(object sender, RoutedEventArgs e)
        {
            SetHudPanelPosition(0, 0, true);
        }

        private void SnapHudPanelToNearestCorner()
        {
            if (RootGrid.ActualWidth <= 0 || RootGrid.ActualHeight <= 0) return;

            var baseLeft = HudPanel.Margin.Left;
            var baseTop = HudPanel.Margin.Top;
            var panelWidth = HudPanel.ActualWidth > 0 ? HudPanel.ActualWidth : HudPanel.Width;
            var panelHeight = HudPanel.ActualHeight > 0 ? HudPanel.ActualHeight : 80;
            var rightX = RootGrid.ActualWidth - baseLeft - HudPanel.Margin.Right - panelWidth;
            var bottomY = RootGrid.ActualHeight - baseTop - HudPanel.Margin.Bottom - panelHeight;
            const double snapDistance = 56;

            var x = hudPanelTransform.X;
            var y = hudPanelTransform.Y;
            var snapLeft = Math.Abs(x) <= snapDistance;
            var snapRight = Math.Abs(x - rightX) <= snapDistance;
            var snapTop = Math.Abs(y) <= snapDistance;
            var snapBottom = Math.Abs(y - bottomY) <= snapDistance;

            if (snapLeft || snapRight) x = snapRight && !snapLeft ? rightX : 0;
            if (snapTop || snapBottom) y = snapBottom && !snapTop ? bottomY : 0;
            SetHudPanelPosition(x, y, false);
        }

        private void RestoreHudPanelPosition()
        {
            var values = ApplicationData.Current.LocalSettings.Values;
            var x = ReadLocalDouble(values, HudPanelXSetting);
            var y = ReadLocalDouble(values, HudPanelYSetting);
            SetHudPanelPosition(x, y, false);
        }

        private static double ReadLocalDouble(IDictionary<string, object> values, string key)
        {
            if (!values.TryGetValue(key, out var value) || value == null) return 0;
            if (value is double number) return number;
            return double.TryParse(value.ToString(), out number) ? number : 0;
        }

        private void SaveHudPanelPosition()
        {
            var values = ApplicationData.Current.LocalSettings.Values;
            values[HudPanelXSetting] = hudPanelTransform.X;
            values[HudPanelYSetting] = hudPanelTransform.Y;
        }

        private void ClampHudPanelPosition()
        {
            SetHudPanelPosition(hudPanelTransform.X, hudPanelTransform.Y, false);
        }

        private void SetHudPanelPosition(double x, double y, bool save)
        {
            // HudPanel starts at Margin=36. Keep at least 24 DIPs of the panel
            // visible even when the host is temporarily smaller than the HUD.
            var baseLeft = HudPanel.Margin.Left;
            var baseTop = HudPanel.Margin.Top;
            if (RootGrid.ActualWidth <= 0 || RootGrid.ActualHeight <= 0)
            {
                hudPanelTransform.X = x;
                hudPanelTransform.Y = y;
                if (save) SaveHudPanelPosition();
                return;
            }
            var panelWidth = HudPanel.ActualWidth > 0 ? HudPanel.ActualWidth : HudPanel.Width;
            var panelHeight = HudPanel.ActualHeight > 0 ? HudPanel.ActualHeight : 80;
            const double visibleEdge = 24;
            var minX = -baseLeft - panelWidth + visibleEdge;
            var minY = -baseTop - panelHeight + visibleEdge;
            var maxX = Math.Max(minX, RootGrid.ActualWidth - baseLeft - visibleEdge);
            var maxY = Math.Max(minY, RootGrid.ActualHeight - baseTop - visibleEdge);
            hudPanelTransform.X = Math.Max(minX, Math.Min(maxX, x));
            hudPanelTransform.Y = Math.Max(minY, Math.Min(maxY, y));
            if (save) SaveHudPanelPosition();
        }

        private async void YoutubeLoadButton_Click(object sender, RoutedEventArgs e)
        {
            await LoadYoutubeAsync();
        }

        private async void YoutubeUrlBox_KeyDown(object sender, KeyRoutedEventArgs e)
        {
            if (e.Key != VirtualKey.Enter) return;
            e.Handled = true;
            await LoadYoutubeAsync();
        }

        private async Task LoadYoutubeAsync()
        {
            var videoId = ExtractYoutubeVideoId(YoutubeUrlBox.Text);
            if (string.IsNullOrWhiteSpace(videoId))
            {
                YoutubeStatusText.Text = "올바른 YouTube 링크 또는 영상 ID를 입력해 주세요";
                return;
            }

            youtubeReady = false;
            youtubePlaying = false;
            YoutubeStatusText.Text = "YouTube 플레이어 불러오는 중…";
            var playerUrl = "https://yummi.duckdns.org/gamebar/youtube-player.html?v=" +
                Uri.EscapeDataString(videoId) + "&volume=" + ((int)Math.Round(youtubeVolume)).ToString();
            YoutubeWebView.Navigate(new Uri(playerUrl));
            await Task.CompletedTask;
        }

        private void YoutubeWebView_NavigationCompleted(WebView sender, WebViewNavigationCompletedEventArgs args)
        {
            if (!args.IsSuccess)
            {
                youtubeReady = false;
                YoutubeStatusText.Text = "YouTube 플레이어를 불러오지 못했습니다";
                return;
            }
            YoutubeStatusText.Text = "YouTube 플레이어 초기화 중…";
        }

        private void YoutubeWebView_ScriptNotify(object sender, NotifyEventArgs e)
        {
            var message = e.Value ?? "";
            var separator = message.IndexOf(':');
            var kind = separator >= 0 ? message.Substring(0, separator) : message;
            var value = separator >= 0 ? message.Substring(separator + 1) : "";

            if (kind == "ready")
            {
                youtubeReady = true;
                YoutubeStatusText.Text = "영상 준비됨 · Game Bar가 열려 있을 때 조작할 수 있습니다";
                return;
            }

            if (kind == "state")
            {
                if (value == "1") youtubePlaying = true;
                else if (value == "0" || value == "2") youtubePlaying = false;
                return;
            }

            if (kind == "error")
            {
                youtubeReady = false;
                youtubePlaying = false;
                YoutubeStatusText.Text = YoutubePlayerErrorText(value);
            }
        }

        private static string YoutubePlayerErrorText(string code)
        {
            switch (code)
            {
                case "101":
                case "150":
                    return "이 영상은 외부 플레이어 재생이 제한되어 있습니다";
                case "153":
                    return "YouTube 플레이어 인증 정보를 확인하지 못했습니다";
                case "2":
                    return "YouTube 영상 ID가 올바르지 않습니다";
                case "5":
                    return "YouTube HTML5 플레이어 오류가 발생했습니다";
                case "100":
                    return "삭제되었거나 비공개인 영상입니다";
                default:
                    return "YouTube 플레이어 오류 · " + code;
            }
        }

        private async void YoutubePlayButton_Click(object sender, RoutedEventArgs e)
        {
            if (await InvokeYoutubeAsync("yummiPlay", "재생")) youtubePlaying = true;
        }

        private async void YoutubePauseButton_Click(object sender, RoutedEventArgs e)
        {
            if (await InvokeYoutubeAsync("yummiPause", "일시정지")) youtubePlaying = false;
        }

        private async Task ToggleYoutubePlaybackAsync()
        {
            if (youtubePlaying)
            {
                if (await InvokeYoutubeAsync("yummiPause", "일시정지 · Space")) youtubePlaying = false;
            }
            else
            {
                if (await InvokeYoutubeAsync("yummiPlay", "재생 · Space")) youtubePlaying = true;
            }
        }

        private async void YoutubeVolumeSlider_ValueChanged(object sender, RangeBaseValueChangedEventArgs e)
        {
            youtubeVolume = e.NewValue;
            if (!youtubeReady || YoutubeWebView == null) return;
            try
            {
                await YoutubeWebView.InvokeScriptAsync("yummiSetVolume", new[] { ((int)Math.Round(youtubeVolume)).ToString() });
            }
            catch { }
        }

        private void YoutubeCollapseButton_Click(object sender, RoutedEventArgs e)
        {
            youtubeCollapsed = !youtubeCollapsed;
            YoutubeBody.Visibility = youtubeCollapsed ? Visibility.Collapsed : Visibility.Visible;
            YoutubeCollapseButton.Content = youtubeCollapsed ? "펼치기" : "접기";
        }

        private async Task<bool> InvokeYoutubeAsync(string functionName, string actionLabel)
        {
            if (!youtubeReady)
            {
                YoutubeStatusText.Text = "먼저 영상을 불러와 주세요";
                return false;
            }
            try
            {
                await YoutubeWebView.InvokeScriptAsync(functionName, new string[0]);
                YoutubeStatusText.Text = actionLabel;
                return true;
            }
            catch (Exception ex)
            {
                YoutubeStatusText.Text = actionLabel + " 실패 · " + ErrorCode(ex);
                return false;
            }
        }

        private static string ExtractYoutubeVideoId(string input)
        {
            var raw = (input ?? "").Trim();
            if (Regex.IsMatch(raw, "^[A-Za-z0-9_-]{11}$")) return raw;

            Uri uri;
            if (!Uri.TryCreate(raw, UriKind.Absolute, out uri))
            {
                if (!Uri.TryCreate("https://" + raw, UriKind.Absolute, out uri)) return null;
            }

            var host = uri.Host.ToLowerInvariant();
            string candidate = null;
            if (host == "youtu.be" || host.EndsWith(".youtu.be"))
            {
                candidate = uri.AbsolutePath.Trim('/').Split('/').FirstOrDefault();
            }
            else if (host == "youtube.com" || host.EndsWith(".youtube.com") || host == "youtube-nocookie.com" || host.EndsWith(".youtube-nocookie.com"))
            {
                var path = uri.AbsolutePath.Trim('/').Split(new[] { '/' }, StringSplitOptions.RemoveEmptyEntries);
                if (path.Length >= 2 && (path[0] == "embed" || path[0] == "shorts" || path[0] == "live")) candidate = path[1];
                if (candidate == null)
                {
                    var match = Regex.Match(uri.Query ?? "", "(?:^|[?&])v=([A-Za-z0-9_-]{11})(?:&|$)");
                    if (match.Success) candidate = match.Groups[1].Value;
                }
            }
            return candidate != null && Regex.IsMatch(candidate, "^[A-Za-z0-9_-]{11}$") ? candidate : null;
        }



        private static string ErrorCode(Exception error)
        {
            var native = error as Win32Exception;
            return native != null ? "Windows 오류 " + native.NativeErrorCode :
                error.GetType().Name + " 0x" + error.HResult.ToString("X8");
        }

        private void SetOffline(Exception error)
        {
            PhaseText.Text = "Offline";
            RiotIdText.Text = "유미 LCU Agent 연결을 확인해 주세요";
            PartyText.Text = "파티 정보 없음";
            var failures = error as AggregateException;
            BridgeText.Text = failures == null ? "에이전트 연결 실패 · " + ErrorCode(error) :
                "파이프 " + ErrorCode(failures.InnerExceptions[0]) + " · TCP " + ErrorCode(failures.InnerExceptions[1]);
            bridgeStatus = "offline";
            bridgeError = BridgeText.Text;
            StatusDot.Fill = new SolidColorBrush(Color.FromArgb(255, 122, 106, 134));
            SaveStatus();
        }
    }
}
