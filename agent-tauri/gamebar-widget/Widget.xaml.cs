using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Linq;
using System.Text;
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
        private readonly DispatcherTimer nativeInputTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(100) };
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
        private FrameworkElement resizedPanel;
        private FrameworkElement resizedBody;
        private uint? panelResizePointerId;
        private Windows.Foundation.Point panelResizeStartPoint;
        private double panelResizeStartWidth;
        private double panelResizeStartHeight;
        private double panelResizeStartX;
        private double panelResizeStartY;
        private string panelResizeMode;
        private string panelResizeKey;
        private bool panelChromePinnedOffsetApplied;
        private const double PanelTitleBarHeight = 24.0;
        private const string LcuPanelXSetting = "lcu-panel-x";
        private const string LcuPanelYSetting = "lcu-panel-y";
        private const string LcuPanelWidthSetting = "lcu-panel-width";
        private const string LcuPanelHeightSetting = "lcu-panel-height";
        private const string PartyPanelXSetting = "party-panel-x";
        private const string PartyPanelYSetting = "party-panel-y";
        private const string PartyPanelWidthSetting = "party-panel-width";
        private const string PartyPanelHeightSetting = "party-panel-height";
        private const string YoutubePanelXSetting = "youtube-panel-x";
        private const string YoutubePanelYSetting = "youtube-panel-y";
        private const string YoutubePanelWidthSetting = "youtube-panel-width";
        private const string YoutubePanelHeightSetting = "youtube-panel-height";
        private const string ClickNativePanelXSetting = "click-native-panel-x";
        private const string ClickNativePanelYSetting = "click-native-panel-y";
        private const string ClickNativePanelWidthSetting = "click-native-panel-width";
        private const string ClickNativePanelHeightSetting = "click-native-panel-height";
        private const string CommandPipeName = @"LOCAL\YummiLcuGameBarCommandV1";
        private bool syncingNativeInput;
        private ulong lastNativeInputSequence;
        private string lastAgentCommandTransport = "none";
        private string lastAgentPipeError = "";
        private string lastAgentTcpError = "";
        private double lastGameBarPointerLocalX;
        private double lastGameBarPointerLocalY;
        private long lastGameBarPointerAtMs;

        public Widget()
        {
            InitializeComponent();
            ClickNativeResult.Text = "Agent native input initializing";
            RootGrid.AddHandler(UIElement.PointerMovedEvent, new PointerEventHandler(RootGrid_PointerMoved), true);
            nativeInputTimer.Tick += async (_, __) => await SyncExternalClickTestAsync(true);
            SetVersionText();
            RestorePanelPositions();
            RestorePanelSizes();
            SizeChanged += (_, __) => ClampAllPanelPositions();
            timer.Tick += async (_, __) => await RefreshAsync();
            Loaded += async (_, __) =>
            {
                App.LogStage("hud-loaded");
                Window.Current.CoreWindow.KeyDown += CoreWindowKeyDown;
                Window.Current.CoreWindow.PointerMoved += CoreWindowPointerMoved;
                Window.Current.CoreWindow.PointerPressed += CoreWindowPointerChanged;
                Window.Current.CoreWindow.PointerReleased += CoreWindowPointerChanged;
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
                nativeInputTimer.Start();
                timer.Start();
                await RefreshAsync();
                App.LogStage("hud-ready");
            };
            Unloaded += (_, __) =>
            {
                Window.Current.CoreWindow.KeyDown -= CoreWindowKeyDown;
                Window.Current.CoreWindow.PointerMoved -= CoreWindowPointerMoved;
                Window.Current.CoreWindow.PointerPressed -= CoreWindowPointerChanged;
                Window.Current.CoreWindow.PointerReleased -= CoreWindowPointerChanged;
                ApplicationData.Current.DataChanged -= PreferencesChanged;
                nativeInputTimer.Stop();
                _ = SyncExternalClickTestAsync(false);
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
            UpdatePanelChrome();
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
                nativeInputTimer.Start();
                _ = SyncExternalClickTestAsync(true);
            }
            else
            {
                ClickNativeResult.Text = "Agent native input unavailable · widget=null";
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
            UpdatePanelChrome();
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
                // Game Bar is a UWP/AppContainer process while the Agent is a desktop app.
                // Cross-AppContainer named pipes are not supported reliably here, so the
                // loopback endpoint is the canonical bridge for both snapshots and commands.
                string line = await ReadLoopbackSnapshotAsync();
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
                await socket.ConnectAsync(new HostName("127.0.0.1"), "19514").AsTask(timeout.Token);
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


        private void UpdateGameBarPointer(Windows.Foundation.Point point)
        {
            lastGameBarPointerLocalX = point.X;
            lastGameBarPointerLocalY = point.Y;
            lastGameBarPointerAtMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        }

        private void RootGrid_PointerMoved(object sender, PointerRoutedEventArgs e)
        {
            try { UpdateGameBarPointer(e.GetCurrentPoint(RootGrid).Position); }
            catch { }
        }

        private void CoreWindowPointerMoved(CoreWindow sender, PointerEventArgs args)
        {
            try { UpdateGameBarPointer(args.CurrentPoint.Position); }
            catch { }
        }

        private void CoreWindowPointerChanged(CoreWindow sender, PointerEventArgs args)
        {
            try { UpdateGameBarPointer(args.CurrentPoint.Position); }
            catch { }
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

        private void PanelMoveBar_PointerPressed(object sender, PointerRoutedEventArgs e)
        {
            if (panelResizePointerId.HasValue || IsPointerFromButton(e.OriginalSource as DependencyObject)) return;
            var key = (sender as FrameworkElement)?.Tag as string;
            BeginPanelDrag(GetPanel(key), e, sender as UIElement);
        }

        private bool IsPointerFromButton(DependencyObject source)
        {
            for (var current = source; current != null; current = VisualTreeHelper.GetParent(current))
            {
                if (current is Button) return true;
                if (current == RootGrid) break;
            }
            return false;
        }

        private void PanelEyeButton_Click(object sender, RoutedEventArgs e)
        {
            var key = (sender as FrameworkElement)?.Tag as string;
            if (string.IsNullOrWhiteSpace(key)) return;
            SetPanelExpanded(key, !GetPanelExpanded(key));
            UpdatePanelChrome();
            ClampAllPanelPositions();
        }

        private void PanelCloseButton_Click(object sender, RoutedEventArgs e)
        {
            var key = (sender as FrameworkElement)?.Tag as string;
            if (string.IsNullOrWhiteSpace(key)) return;
            SetPanelVisible(key, false);
            UpdatePanelChrome();
        }

        private void PanelMoveBar_PointerMoved(object sender, PointerRoutedEventArgs e)
        {
            ContinuePanelDrag(e, sender as UIElement);
        }

        private void PanelMoveBar_PointerReleased(object sender, PointerRoutedEventArgs e)
        {
            EndPanelDrag(e, sender as UIElement, true);
        }

        private void PanelMoveBar_PointerCanceled(object sender, PointerRoutedEventArgs e)
        {
            EndPanelDrag(e, sender as UIElement, false);
        }

        private void PanelMoveBar_PointerCaptureLost(object sender, PointerRoutedEventArgs e)
        {
            if (panelDragPointerId == e.Pointer.PointerId) ClearPanelDrag();
        }

        private void PanelResizeBorder_PointerPressed(object sender, PointerRoutedEventArgs e)
        {
            var target = sender as FrameworkElement;
            var tag = target?.Tag as string;
            if (string.IsNullOrWhiteSpace(tag)) return;
            var parts = tag.Split(':');
            if (parts.Length != 2) return;
            BeginPanelResize(parts[0], parts[1], target, e);
        }

        private void BeginPanelResize(string key, string mode, UIElement target, PointerRoutedEventArgs e)
        {
            if (target == null || panelDragPointerId.HasValue) return;
            var panel = GetPanel(key);
            var body = GetPanelBody(key);
            if (panel == null || body == null || !GetPanelExpanded(key)) return;
            var point = e.GetCurrentPoint(RootGrid);
            if (!point.Properties.IsLeftButtonPressed) return;

            resizedPanel = panel;
            resizedBody = body;
            panelResizeKey = key;
            panelResizeMode = mode;
            panelResizePointerId = e.Pointer.PointerId;
            panelResizeStartPoint = point.Position;
            panelResizeStartWidth = panel.ActualWidth > 0 ? panel.ActualWidth : panel.Width;
            panelResizeStartHeight = body.ActualHeight > 0 ? body.ActualHeight : body.Height;
            var transform = panel.RenderTransform as TranslateTransform;
            panelResizeStartX = transform?.X ?? 0;
            panelResizeStartY = transform?.Y ?? 0;
            target.CapturePointer(e.Pointer);
            SetResizeCursor(mode);
            e.Handled = true;
        }

        private void PanelResizeBorder_PointerMoved(object sender, PointerRoutedEventArgs e)
        {
            if (panelResizePointerId != e.Pointer.PointerId || resizedPanel == null || resizedBody == null) return;
            var point = e.GetCurrentPoint(RootGrid);
            if (!point.Properties.IsLeftButtonPressed)
            {
                EndPanelResize(e, sender as UIElement, true);
                return;
            }

            var deltaX = point.Position.X - panelResizeStartPoint.X;
            var deltaY = point.Position.Y - panelResizeStartPoint.Y;
            var fromLeft = panelResizeMode == "left" || panelResizeMode == "topleft" || panelResizeMode == "bottomleft";
            var fromRight = panelResizeMode == "right" || panelResizeMode == "topright" || panelResizeMode == "bottomright";
            var fromTop = panelResizeMode == "top" || panelResizeMode == "topleft" || panelResizeMode == "topright";
            var fromBottom = panelResizeMode == "bottom" || panelResizeMode == "bottomleft" || panelResizeMode == "bottomright";
            var width = panelResizeStartWidth + (fromRight ? deltaX : fromLeft ? -deltaX : 0);
            var height = panelResizeStartHeight + (fromBottom ? deltaY : fromTop ? -deltaY : 0);
            SetPanelSize(panelResizeKey, width, height, false);
            var appliedWidth = resizedPanel.Width;
            var appliedHeight = resizedBody.Height;
            var x = panelResizeStartX + (fromLeft ? panelResizeStartWidth - appliedWidth : 0);
            var y = panelResizeStartY + (fromTop ? panelResizeStartHeight - appliedHeight : 0);
            SetPanelPosition(resizedPanel, resizedPanel.RenderTransform as TranslateTransform, x, y, false);
            e.Handled = true;
        }

        private void PanelResizeBorder_PointerReleased(object sender, PointerRoutedEventArgs e)
        {
            EndPanelResize(e, sender as UIElement, true);
        }

        private void PanelResizeBorder_PointerCanceled(object sender, PointerRoutedEventArgs e)
        {
            EndPanelResize(e, sender as UIElement, false);
        }

        private void PanelResizeBorder_PointerCaptureLost(object sender, PointerRoutedEventArgs e)
        {
            if (panelResizePointerId != e.Pointer.PointerId) return;
            ClearPanelResize();
            SetResizeCursor(null);
        }

        private void EndPanelResize(PointerRoutedEventArgs e, UIElement captureTarget, bool save)
        {
            if (panelResizePointerId != e.Pointer.PointerId) return;
            try { captureTarget?.ReleasePointerCapture(e.Pointer); } catch { }
            if (save && !string.IsNullOrWhiteSpace(panelResizeKey)) SavePanelSize(panelResizeKey);
            ClearPanelResize();
            SetResizeCursor(null);
            ClampAllPanelPositions();
            e.Handled = true;
        }

        private void ClearPanelResize()
        {
            resizedPanel = null;
            resizedBody = null;
            panelResizePointerId = null;
            panelResizeMode = null;
            panelResizeKey = null;
        }

        private void ResizeBorder_PointerEntered(object sender, PointerRoutedEventArgs e)
        {
            if (panelResizePointerId.HasValue) return;
            var tag = (sender as FrameworkElement)?.Tag as string;
            var parts = tag?.Split(':');
            if (parts != null && parts.Length == 2) SetResizeCursor(parts[1]);
        }

        private void ResizeBorder_PointerExited(object sender, PointerRoutedEventArgs e)
        {
            if (!panelResizePointerId.HasValue) SetResizeCursor(null);
        }

        private static void SetResizeCursor(string mode)
        {
            try
            {
                var type = CoreCursorType.Arrow;
                if (mode == "left" || mode == "right") type = CoreCursorType.SizeWestEast;
                else if (mode == "top" || mode == "bottom") type = CoreCursorType.SizeNorthSouth;
                else if (mode == "topleft" || mode == "bottomright") type = CoreCursorType.SizeNorthwestSoutheast;
                else if (mode == "topright" || mode == "bottomleft") type = CoreCursorType.SizeNortheastSouthwest;
                Window.Current.CoreWindow.PointerCursor = new CoreCursor(type, 0);
            }
            catch { }
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
            RestorePanelPosition(ClickNativePanel, ClickNativePanelTransform, ClickNativePanelXSetting, ClickNativePanelYSetting);
        }

        private void RestorePanelSizes()
        {
            var values = ApplicationData.Current.LocalSettings.Values;
            RestorePanelSize("lcu", values, LcuPanelWidthSetting, LcuPanelHeightSetting, 240, 145);
            RestorePanelSize("party", values, PartyPanelWidthSetting, PartyPanelHeightSetting, 240, 70);
            RestorePanelSize("youtube", values, YoutubePanelWidthSetting, YoutubePanelHeightSetting, 340, 191);
            RestorePanelSize("click-native", values, ClickNativePanelWidthSetting, ClickNativePanelHeightSetting, 250, 92);
        }

        private void RestorePanelSize(string key, IDictionary<string, object> values, string widthKey, string heightKey, double defaultWidth, double defaultHeight)
        {
            var width = ReadLocalDouble(values, widthKey);
            var height = ReadLocalDouble(values, heightKey);
            if (width <= 0) width = defaultWidth;
            if (height <= 0) height = defaultHeight;
            SetPanelSize(key, width, height, false);
        }

        private void SetPanelSize(string key, double requestedWidth, double requestedHeight, bool save)
        {
            var panel = GetPanel(key);
            var body = GetPanelBody(key);
            if (panel == null || body == null) return;

            double minWidth = 160, maxWidth = 1280, minHeight = 48, maxHeight = 720;
            if (key == "lcu") { minWidth = 190; maxWidth = 900; minHeight = 100; maxHeight = 600; }
            else if (key == "party") { minWidth = 160; maxWidth = 800; minHeight = 48; maxHeight = 500; }
            else if (key == "youtube") { minWidth = 220; maxWidth = 1280; minHeight = 124; maxHeight = 720; }
            else if (key.StartsWith("click-")) { minWidth = 190; maxWidth = 700; minHeight = 70; maxHeight = 300; }

            panel.Width = Math.Max(minWidth, Math.Min(maxWidth, requestedWidth));
            body.Height = Math.Max(minHeight, Math.Min(maxHeight, requestedHeight));
            if (save) SavePanelSize(key);
        }

        private void SavePanelSize(string key)
        {
            var values = ApplicationData.Current.LocalSettings.Values;
            var panel = GetPanel(key);
            var body = GetPanelBody(key);
            if (panel == null || body == null) return;
            if (key == "lcu")
            {
                values[LcuPanelWidthSetting] = panel.Width;
                values[LcuPanelHeightSetting] = body.Height;
            }
            else if (key == "party")
            {
                values[PartyPanelWidthSetting] = panel.Width;
                values[PartyPanelHeightSetting] = body.Height;
            }
            else if (key == "youtube")
            {
                values[YoutubePanelWidthSetting] = panel.Width;
                values[YoutubePanelHeightSetting] = body.Height;
            }
            else if (key == "click-native") { values[ClickNativePanelWidthSetting] = panel.Width; values[ClickNativePanelHeightSetting] = body.Height; }
        }

        private FrameworkElement GetPanel(string key)
        {
            if (key == "lcu") return LcuPanel;
            if (key == "party") return PartyPanel;
            if (key == "youtube") return YoutubePanel;
            if (key == "click-native") return ClickNativePanel;
            return null;
        }

        private FrameworkElement GetPanelBody(string key)
        {
            if (key == "lcu") return LcuBody;
            if (key == "party") return PartyBody;
            if (key == "youtube") return YoutubeVideoHost;
            if (key == "click-native") return ClickNativeBody;
            return null;
        }

        private bool GetPanelVisible(string key)
        {
            if (key == "lcu") return HudPreferences.LcuPanelVisible;
            if (key == "party") return HudPreferences.PartyPanelVisible;
            if (key == "youtube") return YoutubePreferences.Visible;
            return false;
        }

        private void SetPanelVisible(string key, bool visible)
        {
            if (key == "lcu") HudPreferences.LcuPanelVisible = visible;
            else if (key == "party") HudPreferences.PartyPanelVisible = visible;
            else if (key == "youtube") YoutubePreferences.Visible = visible;
        }

        private bool GetPanelExpanded(string key)
        {
            if (key == "lcu") return HudPreferences.LcuPanelExpanded;
            if (key == "party") return HudPreferences.PartyPanelExpanded;
            if (key == "youtube") return HudPreferences.YoutubePanelExpanded;
            return true;
        }

        private void SetPanelExpanded(string key, bool expanded)
        {
            if (key == "lcu") HudPreferences.LcuPanelExpanded = expanded;
            else if (key == "party") HudPreferences.PartyPanelExpanded = expanded;
            else if (key == "youtube") HudPreferences.YoutubePanelExpanded = expanded;
        }

        private void UpdatePanelChrome()
        {
            bool pinnedOnly = widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly;
            ApplyPinnedBodyPositionOffset(pinnedOnly);
            ApplyPanelChrome("lcu", LcuPanel, LcuMoveBar, LcuBody, LcuEyeButton, LcuResizeHandles, pinnedOnly);
            ApplyPanelChrome("party", PartyPanel, PartyMoveBar, PartyBody, PartyEyeButton, PartyResizeHandles, pinnedOnly);
            ApplyPanelChrome("youtube", YoutubePanel, YoutubeMoveBar, YoutubeVideoHost, YoutubeEyeButton, YoutubeResizeHandles, pinnedOnly);
            ApplyClickTestPanelChrome(ClickNativeMoveBar, ClickNativeResizeHandles, pinnedOnly);
        }

        private static void ApplyClickTestPanelChrome(FrameworkElement moveBar, FrameworkElement resizeHandles, bool pinnedOnly)
        {
            moveBar.Visibility = pinnedOnly ? Visibility.Collapsed : Visibility.Visible;
            resizeHandles.Visibility = pinnedOnly ? Visibility.Collapsed : Visibility.Visible;
        }

        private void ApplyPinnedBodyPositionOffset(bool pinnedOnly)
        {
            if (panelChromePinnedOffsetApplied == pinnedOnly) return;

            // Stored Y is the edit-mode panel position, where the 24 DIP title
            // bar sits above the body. When the title bar collapses in pinned
            // mode, move the whole panel down by the same amount so the body
            // remains at the exact same screen Y. Reverse it when Win+G opens.
            var delta = pinnedOnly ? PanelTitleBarHeight : -PanelTitleBarHeight;
            LcuPanelTransform.Y += delta;
            PartyPanelTransform.Y += delta;
            YoutubePanelTransform.Y += delta;
            ClickNativePanelTransform.Y += delta;
            panelChromePinnedOffsetApplied = pinnedOnly;
        }

        private void ApplyPanelChrome(string key, FrameworkElement panel, FrameworkElement moveBar, FrameworkElement body, Control eyeButton,
            FrameworkElement resizeHandles, bool pinnedOnly)
        {
            bool visible = GetPanelVisible(key);
            bool expanded = GetPanelExpanded(key);
            panel.Visibility = visible && (!pinnedOnly || expanded) ? Visibility.Visible : Visibility.Collapsed;
            moveBar.Visibility = visible && !pinnedOnly ? Visibility.Visible : Visibility.Collapsed;
            body.Visibility = expanded ? Visibility.Visible : Visibility.Collapsed;
            eyeButton.Opacity = expanded ? 1.0 : 0.5;
            ToolTipService.SetToolTip(eyeButton, expanded ? "내용 가리기" : "내용 표시");
            resizeHandles.Visibility = visible && expanded && !pinnedOnly ? Visibility.Visible : Visibility.Collapsed;
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
            else if (panel == ClickNativePanel) { values[ClickNativePanelXSetting] = transform.X; values[ClickNativePanelYSetting] = transform.Y; }
        }

        private void ClampAllPanelPositions()
        {
            SetPanelPosition(LcuPanel, LcuPanelTransform, LcuPanelTransform.X, LcuPanelTransform.Y, false);
            SetPanelPosition(PartyPanel, PartyPanelTransform, PartyPanelTransform.X, PartyPanelTransform.Y, false);
            SetPanelPosition(YoutubePanel, YoutubePanelTransform, YoutubePanelTransform.X, YoutubePanelTransform.Y, false);
            SetPanelPosition(ClickNativePanel, ClickNativePanelTransform, ClickNativePanelTransform.X, ClickNativePanelTransform.Y, false);
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

        private async Task SyncExternalClickTestAsync(bool active)
        {
            if (syncingNativeInput) return;
            if (widget == null)
            {
                ClickNativeResult.Text = "Agent native input unavailable · widget=null";
                return;
            }
            syncingNativeInput = true;
            try
            {
                var requestedActive = active && HudPreferences.Visible && HudPanel.Visibility == Visibility.Visible
                    && ClickNativePanel.Visibility == Visibility.Visible;
                var geometryReady = NativeHookHitbox.ActualWidth > 0 && NativeHookHitbox.ActualHeight > 0;
                var effectiveActive = requestedActive && geometryReady;
                var command = new JsonObject
                {
                    ["command"] = JsonValue.CreateStringValue("sync_external_click_test"),
                    ["active"] = JsonValue.CreateBooleanValue(effectiveActive),
                };
                if (lastGameBarPointerAtMs > 0)
                {
                    var bounds = widget.WindowBounds;
                    var scale = Math.Max(1.0, DisplayInformation.GetForCurrentView().RawPixelsPerViewPixel);
                    command["gamebarCursorX"] = JsonValue.CreateNumberValue((bounds.X + lastGameBarPointerLocalX) * scale);
                    command["gamebarCursorY"] = JsonValue.CreateNumberValue((bounds.Y + lastGameBarPointerLocalY) * scale);
                    command["gamebarAltCursorX"] = JsonValue.CreateNumberValue(bounds.X + (lastGameBarPointerLocalX * scale));
                    command["gamebarAltCursorY"] = JsonValue.CreateNumberValue(bounds.Y + (lastGameBarPointerLocalY * scale));
                    command["gamebarCursorAtMs"] = JsonValue.CreateNumberValue(lastGameBarPointerAtMs);
                }
                if (effectiveActive)
                {
                    var local = NativeHookHitbox.TransformToVisual(RootGrid)
                        .TransformPoint(new Windows.Foundation.Point(0, 0));
                    var bounds = widget.WindowBounds;
                    var scale = Math.Max(1.0, DisplayInformation.GetForCurrentView().RawPixelsPerViewPixel);
                    // WindowBounds does not document whether X/Y are already raw screen pixels.
                    // Send both plausible coordinate mappings; the Agent accepts either hitbox.
                    command["x"] = JsonValue.CreateNumberValue((bounds.X + local.X) * scale);
                    command["y"] = JsonValue.CreateNumberValue((bounds.Y + local.Y) * scale);
                    command["width"] = JsonValue.CreateNumberValue(NativeHookHitbox.ActualWidth * scale);
                    command["height"] = JsonValue.CreateNumberValue(NativeHookHitbox.ActualHeight * scale);
                    command["altX"] = JsonValue.CreateNumberValue(bounds.X + (local.X * scale));
                    command["altY"] = JsonValue.CreateNumberValue(bounds.Y + (local.Y * scale));
                    command["altWidth"] = JsonValue.CreateNumberValue(NativeHookHitbox.ActualWidth * scale);
                    command["altHeight"] = JsonValue.CreateNumberValue(NativeHookHitbox.ActualHeight * scale);
                }

                var rawResponse = await SendAgentCommandAsync(command.Stringify());
                var response = JsonObject.Parse(rawResponse);
                if (!response.GetNamedBoolean("ok", false))
                {
                    ClickNativeResult.Text = "Agent native · " + response.GetNamedString("message", "sync failed");
                    return;
                }
                var sequence = (ulong)Math.Max(0, response.GetNamedNumber("sequence", 0));
                var clickCount = (ulong)Math.Max(0, response.GetNamedNumber("clickCount", 0));
                var eventName = response.GetNamedString("event", "");
                var cursorX = (int)response.GetNamedNumber("cursorX", 0);
                var cursorY = (int)response.GetNamedNumber("cursorY", 0);
                var monitorX = (int)response.GetNamedNumber("monitorX", 0);
                var monitorY = (int)response.GetNamedNumber("monitorY", 0);
                var rectX = (int)Math.Round(response.GetNamedNumber("rectX", 0));
                var rectY = (int)Math.Round(response.GetNamedNumber("rectY", 0));
                var altRectX = (int)Math.Round(response.GetNamedNumber("altRectX", 0));
                var altRectY = (int)Math.Round(response.GetNamedNumber("altRectY", 0));
                var pollCount = (ulong)Math.Max(0, response.GetNamedNumber("pollCount", 0));
                var pollAgeMs = (ulong)Math.Max(0, response.GetNamedNumber("pollAgeMs", 0));
                var cursorSource = response.GetNamedString("cursorSource", "?");
                var agentPid = (uint)Math.Max(0, response.GetNamedNumber("agentPid", 0));
                var agentBuild = response.GetNamedString("agentBuildId", "?");
                var eventX = (int)response.GetNamedNumber("eventX", 0);
                var eventY = (int)response.GetNamedNumber("eventY", 0);
                var matched = response.GetNamedString("matchedGeometry", "?");
                var responseActive = response.GetNamedBoolean("active", false);
                var rectWidth = response.GetNamedNumber("rectWidth", 0);
                var rectHeight = response.GetNamedNumber("rectHeight", 0);
                var altRectWidth = response.GetNamedNumber("altRectWidth", 0);
                var altRectHeight = response.GetNamedNumber("altRectHeight", 0);
                var eventAtMs = (ulong)Math.Max(0, response.GetNamedNumber("eventAtMs", 0));
                var releaseLabel = response.GetNamedString("agentReleaseLabel", "?");
                var agentCommit = response.GetNamedString("agentCommit", "?");
                if (sequence != lastNativeInputSequence) lastNativeInputSequence = sequence;

                ClickNativeResult.Text =
                    "transport=" + lastAgentCommandTransport + " pipeErr=" + lastAgentPipeError + " tcpErr=" + lastAgentTcpError +
                    " requestedActive=" + requestedActive + " geometryReady=" + geometryReady +
                    " hitbox=" + NativeHookHitbox.ActualWidth.ToString("0.##") + "x" + NativeHookHitbox.ActualHeight.ToString("0.##") + "\n" +
                    "agent pid=" + agentPid + " build=" + agentBuild + " release=" + releaseLabel + " commit=" + agentCommit + "\n" +
                    "active=" + responseActive + " seq=" + sequence + " clicks=" + clickCount + " event=" + eventName +
                    " eventXY=" + eventX + "," + eventY + " eventAt=" + eventAtMs + " matched=" + matched + "\n" +
                    "cursor=" + cursorX + "," + cursorY + " source=" + cursorSource + " monitor=" + monitorX + "," + monitorY +
                    " pollCount=" + pollCount + " pollAgeMs=" + pollAgeMs + "\n" +
                    "rect=" + rectX + "," + rectY + " " + rectWidth.ToString("0.##") + "x" + rectHeight.ToString("0.##") +
                    " altRect=" + altRectX + "," + altRectY + " " + altRectWidth.ToString("0.##") + "x" + altRectHeight.ToString("0.##") + "\n" +
                    "raw=" + rawResponse;
            }
            catch (Exception ex)
            {
                ClickNativeResult.Text = "Agent native input unavailable · " + ErrorCode(ex);
            }
            finally
            {
                syncingNativeInput = false;
            }
        }

        private async Task<string> SendAgentCommandAsync(string payload)
        {
            // Desktop Agent <-> UWP Game Bar uses loopback TCP as the canonical
            // transport. Named pipes are intentionally skipped because UWP named
            // pipes are restricted to the same AppContainer app.
            lastAgentPipeError = "";
            try
            {
                using (var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(1)))
                using (var socket = new StreamSocket())
                {
                    await socket.ConnectAsync(new HostName("127.0.0.1"), "19514").AsTask(timeout.Token);
                    using (var output = socket.OutputStream.AsStreamForWrite())
                    using (var input = socket.InputStream.AsStreamForRead())
                    {
                        // Unified endpoint sends the current snapshot first.
                        await NativePipeClient.ReadLineAsync(input, timeout.Token).ConfigureAwait(false);
                        var data = Encoding.UTF8.GetBytes(payload + "\n");
                        await output.WriteAsync(data, 0, data.Length, timeout.Token).ConfigureAwait(false);
                        await output.FlushAsync(timeout.Token).ConfigureAwait(false);
                        var response = await NativePipeClient.ReadLineAsync(input, timeout.Token).ConfigureAwait(false);
                        lastAgentCommandTransport = "tcp:19514";
                        lastAgentTcpError = "";
                        return response;
                    }
                }
            }
            catch (Exception socketError)
            {
                lastAgentTcpError = ErrorCode(socketError);
                lastAgentCommandTransport = "failed";
                throw;
            }
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
            RiotIdText.Text = "유미지지 데스크톱 연결을 확인해 주세요";
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
