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
        private bool youtubePlaying;
        private double youtubeVolume = 60;
        private Microsoft.UI.Xaml.Controls.WebView2 youtubeWebView2;
        private bool youtubeWebView2Unavailable;
        private bool youtubeUsingFallback;
        private string youtubePendingPlayerUrl;
        private string youtubeLoadedVideoId;
        private long youtubeLastActionRequest;
        private FrameworkElement draggedPanel;
        private TranslateTransform draggedPanelTransform;
        private uint? panelDragPointerId;
        private Windows.Foundation.Point panelDragStartPoint;
        private double panelDragStartX;
        private double panelDragStartY;
        private const string LcuPanelXSetting = "lcu-panel-x";
        private const string LcuPanelYSetting = "lcu-panel-y";
        private const string PartyPanelXSetting = "party-panel-x";
        private const string PartyPanelYSetting = "party-panel-y";
        private const string YoutubePanelXSetting = "youtube-panel-x";
        private const string YoutubePanelYSetting = "youtube-panel-y";

        public Widget()
        {
            InitializeComponent();
            SetVersionText();
            RestorePanelPositions();
            SizeChanged += (_, __) => ClampAllPanelPositions();
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


        private void SetVersionText()
        {
            try
            {
                var version = Package.Current.Id.Version;
                VersionText.Text = "v" + version.Major + "." + version.Minor + "." + version.Build;
            }
            catch
            {
                VersionText.Text = "v?";
            }
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

            if (args.VirtualKey == VirtualKey.Space && youtubeReady)
            {
                var focused = FocusManager.GetFocusedElement() as DependencyObject;
                if (focused is TextBox || focused is WebView ||
                    (youtubeWebView2 != null && focused == youtubeWebView2)) return;
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
            ApplyYoutubePreferences();
            if (string.IsNullOrWhiteSpace(ExtractYoutubeVideoId(YoutubePreferences.Video)))
                SetYoutubeInlineState(true, "영상 링크를 입력해 주세요");
        }

        private void ApplyYoutubePreferences()
        {
            var configuredVideo = YoutubePreferences.Video;
            var configuredId = ExtractYoutubeVideoId(configuredVideo);
            var configuredVolume = YoutubePreferences.Volume;
            youtubeVolume = configuredVolume;
            if (youtubeReady) _ = SetYoutubeVolumeAsync(configuredVolume);

            if (!string.IsNullOrWhiteSpace(configuredId) && !string.Equals(configuredId, youtubeLoadedVideoId, StringComparison.Ordinal))
            {
                youtubeLoadedVideoId = configuredId;
                _ = LoadYoutubeVideoAsync(configuredId);
            }

            var actionRequest = YoutubePreferences.ActionRequest;
            if (actionRequest != youtubeLastActionRequest)
            {
                youtubeLastActionRequest = actionRequest;
                if (youtubeReady)
                {
                    var action = YoutubePreferences.Action;
                    if (action == "play") { youtubePlaying = true; _ = InvokeYoutubeAsync("yummiPlay", "재생"); }
                    else if (action == "pause") { youtubePlaying = false; _ = InvokeYoutubeAsync("yummiPause", "일시정지"); }
                }
            }
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

        private void Panel_PointerPressed(object sender, PointerRoutedEventArgs e)
        {
            BeginPanelDrag(sender as FrameworkElement, e, sender as UIElement);
        }

        private void Panel_PointerMoved(object sender, PointerRoutedEventArgs e)
        {
            ContinuePanelDrag(e, sender as UIElement);
        }

        private void Panel_PointerReleased(object sender, PointerRoutedEventArgs e)
        {
            EndPanelDrag(e, sender as UIElement, true);
        }

        private void Panel_PointerCanceled(object sender, PointerRoutedEventArgs e)
        {
            EndPanelDrag(e, sender as UIElement, false);
        }

        private void Panel_PointerCaptureLost(object sender, PointerRoutedEventArgs e)
        {
            if (panelDragPointerId == e.Pointer.PointerId) ClearPanelDrag();
        }

        private void YoutubeDragHandle_PointerPressed(object sender, PointerRoutedEventArgs e)
        {
            BeginPanelDrag(YoutubePanel, e, sender as UIElement);
        }

        private void YoutubeDragHandle_PointerMoved(object sender, PointerRoutedEventArgs e)
        {
            ContinuePanelDrag(e, sender as UIElement);
        }

        private void YoutubeDragHandle_PointerReleased(object sender, PointerRoutedEventArgs e)
        {
            EndPanelDrag(e, sender as UIElement, true);
        }

        private void YoutubeDragHandle_PointerCanceled(object sender, PointerRoutedEventArgs e)
        {
            EndPanelDrag(e, sender as UIElement, false);
        }

        private void YoutubeDragHandle_PointerCaptureLost(object sender, PointerRoutedEventArgs e)
        {
            if (panelDragPointerId == e.Pointer.PointerId) ClearPanelDrag();
        }

        private void BeginPanelDrag(FrameworkElement panel, PointerRoutedEventArgs e, UIElement captureTarget)
        {
            if (panel == null || captureTarget == null) return;
            var point = e.GetCurrentPoint(RootGrid);
            if (!point.Properties.IsLeftButtonPressed) return;
            var transform = panel.RenderTransform as TranslateTransform;
            if (transform == null) return;

            draggedPanel = panel;
            draggedPanelTransform = transform;
            panelDragPointerId = e.Pointer.PointerId;
            panelDragStartPoint = point.Position;
            panelDragStartX = transform.X;
            panelDragStartY = transform.Y;
            captureTarget.CapturePointer(e.Pointer);
            e.Handled = true;
        }

        private void ContinuePanelDrag(PointerRoutedEventArgs e, UIElement captureTarget)
        {
            if (panelDragPointerId != e.Pointer.PointerId || draggedPanel == null || draggedPanelTransform == null) return;
            var point = e.GetCurrentPoint(RootGrid);
            if (!point.Properties.IsLeftButtonPressed)
            {
                EndPanelDrag(e, captureTarget, true);
                return;
            }
            SetPanelPosition(
                draggedPanel,
                draggedPanelTransform,
                panelDragStartX + point.Position.X - panelDragStartPoint.X,
                panelDragStartY + point.Position.Y - panelDragStartPoint.Y,
                false);
            e.Handled = true;
        }

        private void EndPanelDrag(PointerRoutedEventArgs e, UIElement captureTarget, bool save)
        {
            if (panelDragPointerId != e.Pointer.PointerId) return;
            try { captureTarget?.ReleasePointerCapture(e.Pointer); } catch { }
            if (save && draggedPanel != null && draggedPanelTransform != null)
                SavePanelPosition(draggedPanel, draggedPanelTransform);
            ClearPanelDrag();
            e.Handled = true;
        }

        private void ClearPanelDrag()
        {
            draggedPanel = null;
            draggedPanelTransform = null;
            panelDragPointerId = null;
        }

        private void RestorePanelPositions()
        {
            RestorePanelPosition(LcuPanel, LcuPanelTransform, LcuPanelXSetting, LcuPanelYSetting);
            RestorePanelPosition(PartyPanel, PartyPanelTransform, PartyPanelXSetting, PartyPanelYSetting);
            RestorePanelPosition(YoutubePanel, YoutubePanelTransform, YoutubePanelXSetting, YoutubePanelYSetting);
        }

        private void RestorePanelPosition(FrameworkElement panel, TranslateTransform transform, string xKey, string yKey)
        {
            var values = ApplicationData.Current.LocalSettings.Values;
            SetPanelPosition(panel, transform, ReadLocalDouble(values, xKey), ReadLocalDouble(values, yKey), false);
        }

        private static double ReadLocalDouble(IDictionary<string, object> values, string key)
        {
            if (!values.TryGetValue(key, out var value) || value == null) return 0;
            if (value is double number) return number;
            return double.TryParse(value.ToString(), out number) ? number : 0;
        }

        private void SavePanelPosition(FrameworkElement panel, TranslateTransform transform)
        {
            var values = ApplicationData.Current.LocalSettings.Values;
            if (panel == LcuPanel)
            {
                values[LcuPanelXSetting] = transform.X;
                values[LcuPanelYSetting] = transform.Y;
            }
            else if (panel == PartyPanel)
            {
                values[PartyPanelXSetting] = transform.X;
                values[PartyPanelYSetting] = transform.Y;
            }
            else if (panel == YoutubePanel)
            {
                values[YoutubePanelXSetting] = transform.X;
                values[YoutubePanelYSetting] = transform.Y;
            }
        }

        private void ClampAllPanelPositions()
        {
            SetPanelPosition(LcuPanel, LcuPanelTransform, LcuPanelTransform.X, LcuPanelTransform.Y, false);
            SetPanelPosition(PartyPanel, PartyPanelTransform, PartyPanelTransform.X, PartyPanelTransform.Y, false);
            SetPanelPosition(YoutubePanel, YoutubePanelTransform, YoutubePanelTransform.X, YoutubePanelTransform.Y, false);
        }

        private void SetPanelPosition(FrameworkElement panel, TranslateTransform transform, double x, double y, bool save)
        {
            if (panel == null || transform == null) return;
            if (RootGrid.ActualWidth > 0 && RootGrid.ActualHeight > 0)
            {
                var width = panel.ActualWidth > 0 ? panel.ActualWidth : (double.IsNaN(panel.Width) ? 100 : panel.Width);
                var height = panel.ActualHeight > 0 ? panel.ActualHeight : (double.IsNaN(panel.Height) ? 80 : panel.Height);
                const double visibleEdge = 24;
                var baseLeft = panel.Margin.Left;
                var baseTop = panel.Margin.Top;
                var minX = -baseLeft - width + visibleEdge;
                var minY = -baseTop - height + visibleEdge;
                var maxX = Math.Max(minX, RootGrid.ActualWidth - baseLeft - visibleEdge);
                var maxY = Math.Max(minY, RootGrid.ActualHeight - baseTop - visibleEdge);
                x = Math.Max(minX, Math.Min(maxX, x));
                y = Math.Max(minY, Math.Min(maxY, y));
            }
            transform.X = x;
            transform.Y = y;
            if (save) SavePanelPosition(panel, transform);
        }

        private async Task LoadYoutubeVideoAsync(string videoId)
        {
            youtubeReady = false;
            youtubePlaying = false;
            SetYoutubeInlineState(true, "YouTube 영상을 불러오는 중…");
            youtubePendingPlayerUrl = "https://yummi.duckdns.org/gamebar/youtube-player.html?v=" +
                Uri.EscapeDataString(videoId) + "&volume=" + ((int)Math.Round(youtubeVolume)).ToString();

            if (await EnsureYoutubeWebView2Async())
            {
                youtubeUsingFallback = false;
                YoutubeFallbackWebView.Visibility = Visibility.Collapsed;
                youtubeWebView2.Visibility = Visibility.Visible;
                youtubeWebView2.Source = new Uri(youtubePendingPlayerUrl);
                return;
            }

            NavigateYoutubeFallback(youtubePendingPlayerUrl);
        }

        private async Task<bool> EnsureYoutubeWebView2Async()
        {
            if (youtubeWebView2Unavailable) return false;
            if (youtubeWebView2 != null && youtubeWebView2.CoreWebView2 != null) return true;

            try
            {
                if (youtubeWebView2 == null)
                {
                    EnsureWinUiResources();
                    youtubeWebView2 = new Microsoft.UI.Xaml.Controls.WebView2
                    {
                        HorizontalAlignment = HorizontalAlignment.Stretch,
                        VerticalAlignment = VerticalAlignment.Stretch
                    };
                    youtubeWebView2.NavigationCompleted += YoutubeWebView2_NavigationCompleted;
                    YoutubeWebHost.Children.Insert(0, youtubeWebView2);
                }

                await youtubeWebView2.EnsureCoreWebView2Async();
                if (youtubeWebView2.CoreWebView2 == null) throw new InvalidOperationException("CoreWebView2 is unavailable");
                youtubeWebView2.CoreWebView2.Settings.AreDefaultContextMenusEnabled = true;
                youtubeWebView2.CoreWebView2.Settings.AreDevToolsEnabled = false;
                youtubeWebView2.CoreWebView2.Settings.IsStatusBarEnabled = false;
                return true;
            }
            catch (Exception ex)
            {
                youtubeWebView2Unavailable = true;
                youtubeUsingFallback = true;
                if (youtubeWebView2 != null) youtubeWebView2.Visibility = Visibility.Collapsed;
                App.LogStage("youtube-webview2-fallback-" + ErrorCode(ex));
                return false;
            }
        }

        private static void EnsureWinUiResources()
        {
            var resources = Application.Current.Resources;
            foreach (var dictionary in resources.MergedDictionaries)
            {
                if (dictionary is Microsoft.UI.Xaml.Controls.XamlControlsResources) return;
            }
            resources.MergedDictionaries.Add(new Microsoft.UI.Xaml.Controls.XamlControlsResources());
        }

        private void NavigateYoutubeFallback(string playerUrl)
        {
            youtubeUsingFallback = true;
            YoutubeFallbackWebView.Visibility = Visibility.Visible;
            YoutubeFallbackWebView.Navigate(new Uri(playerUrl));
        }

        private void YoutubeWebView2_NavigationCompleted(
            Microsoft.UI.Xaml.Controls.WebView2 sender,
            Microsoft.Web.WebView2.Core.CoreWebView2NavigationCompletedEventArgs args)
        {
            if (!args.IsSuccess)
            {
                youtubeReady = false;
                SetYoutubeInlineState(true, "YouTube 영상을 불러오지 못했습니다.\n링크를 확인해 주세요.");
                return;
            }
            youtubeReady = true;
            SetYoutubeInlineState(false, null);
            _ = SetYoutubeVolumeAsync(youtubeVolume);
        }

        private void YoutubeFallbackWebView_NavigationCompleted(WebView sender, WebViewNavigationCompletedEventArgs args)
        {
            if (!args.IsSuccess)
            {
                youtubeReady = false;
                SetYoutubeInlineState(true, "YouTube 영상을 불러오지 못했습니다.\n링크를 확인해 주세요.");
                return;
            }
            youtubeReady = true;
            SetYoutubeInlineState(false, null);
            _ = SetYoutubeVolumeAsync(youtubeVolume);
        }

        private void SetYoutubeInlineState(bool visible, string message)
        {
            if (YoutubeInlineState == null) return;
            YoutubeInlineState.Visibility = visible ? Visibility.Visible : Visibility.Collapsed;
            if (!string.IsNullOrWhiteSpace(message) && YoutubeInlineStateText != null)
                YoutubeInlineStateText.Text = message;
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

        private async Task SetYoutubeVolumeAsync(double value)
        {
            if (!youtubeReady) return;
            try
            {
                var volume = ((int)Math.Round(Math.Max(0, Math.Min(100, value)))).ToString();
                if (!youtubeUsingFallback && youtubeWebView2 != null && youtubeWebView2.CoreWebView2 != null)
                    await youtubeWebView2.ExecuteScriptAsync("window.yummiSetVolume && window.yummiSetVolume(" + volume + ");");
                else if (youtubeUsingFallback)
                    await YoutubeFallbackWebView.InvokeScriptAsync("yummiSetVolume", new[] { volume });
            }
            catch { }
        }

        private async Task<bool> InvokeYoutubeAsync(string functionName, string actionLabel)
        {
            if (!youtubeReady)
            {
                SetYoutubeInlineState(true, "설정에서 YouTube 영상을 선택해 주세요");
                return false;
            }
            try
            {
                if (!youtubeUsingFallback && youtubeWebView2 != null && youtubeWebView2.CoreWebView2 != null)
                {
                    await youtubeWebView2.ExecuteScriptAsync("window." + functionName + " && window." + functionName + "();");
                }
                else
                {
                    await YoutubeFallbackWebView.InvokeScriptAsync(functionName, new string[0]);
                }
                return true;
            }
            catch (Exception ex)
            {
                SetYoutubeInlineState(true, actionLabel + " 실패 · " + ErrorCode(ex));
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
