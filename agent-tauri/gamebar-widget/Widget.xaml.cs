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
using Windows.UI.Xaml.Media.Animation;
using Windows.UI.Xaml.Media.Imaging;
using Windows.UI.Xaml.Shapes;
using Windows.UI.Xaml.Input;
using Windows.UI.Xaml.Navigation;

namespace YummiGameBarWidget
{
    public sealed partial class Widget : Page
    {
        private readonly DispatcherTimer timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(1) };
        private CancellationTokenSource nativeInputLoopCts;
        private long lastNativeInputSyncOkAtMs;
        private long lastNativeInputStaleLogAtMs;
        private string lastNativeInputKeepalivePayload = "";
        private int nativeInputKeepaliveInFlight;
        private int nativeInputDispatchInFlight;
        private long nativeInputDispatchQueuedAtMs;
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
        private bool youtubeBrowseMode;
        private double youtubePlayerHeightBeforeBrowse;
        private XboxGameBarWidgetNotificationManager notificationManager;
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
        private const string VoiceChatPanelXSetting = "voice-chat-panel-x";
        private const string VoiceChatPanelYSetting = "voice-chat-panel-y";
        private const string CommandPipeName = @"LOCAL\YummiLcuGameBarCommandV1";
        private bool syncingNativeInput;
        private long syncingNativeInputSinceMs;
        private int nativeInputSyncGeneration;
        private ulong lastNativeInputSequence;
        private ulong lastHudToggleSequence;
        private bool hudToggleSequenceInitialized;
        private string lastAgentCommandTransport = "none";
        private string lastAgentPipeError = "";
        private string lastAgentTcpError = "";
        private double lastGameBarPointerLocalX;
        private double lastGameBarPointerLocalY;
        private long lastGameBarPointerAtMs;
        private double clickNativeExpandedHeight = 300;
        private string voiceChatSignature = "";
        private readonly Dictionary<string, BitmapImage> voiceChatAvatarCache = new Dictionary<string, BitmapImage>(StringComparer.Ordinal);
        private readonly Dictionary<string, long> voiceChatFirstSeenAtMs = new Dictionary<string, long>(StringComparer.Ordinal);
        private readonly HashSet<string> voiceChatRenderedKeys = new HashSet<string>(StringComparer.Ordinal);
        private bool voiceChatHasVisibleMessages;
        private bool voiceChatPositionNeedsDefault;
        private bool controlsTutorialShownThisSession;
        private long lastSnapshotSuccessAtMs;
        private int consecutiveSnapshotFailures;
        private long lastSnapshotFailureLogAtMs;
        private long lastTutorialResetGeneration;
        private bool tutorialResetAwaitingUnpin;
        private int tutorialRefreshGeneration;
        private int pinnedCoverageRestoreGeneration;
        private bool pinnedCoverageRestoreInFlight;
        private bool hudLoaded;

        public Widget()
        {
            InitializeComponent();
            CursorMarkersToggle.IsOn = HudPreferences.NativeCursorMarkersVisible;
            ApplyCursorMarkersVisibility(CursorMarkersToggle.IsOn);
            ClickNativeResult.Text = "Agent native input initializing";
            RootGrid.AddHandler(UIElement.PointerMovedEvent, new PointerEventHandler(RootGrid_PointerMoved), true);
            SetVersionText();
            RestorePanelPositions();
            RestorePanelSizes();
            SizeChanged += (_, __) =>
            {
                EnsureDefaultVoiceChatPosition();
                ClampAllPanelPositions();
                UpdatePinTutorial();
                QueuePinnedCoverageRestore("size-changed");
            };
            timer.Tick += async (_, __) => await RefreshAsync();
            Loaded += async (_, __) =>
            {
                App.LogStage("hud-loaded");
                hudLoaded = true;
                StartNativeInputLoop();
                Window.Current.CoreWindow.KeyDown += CoreWindowKeyDown;
                Window.Current.CoreWindow.PointerMoved += CoreWindowPointerMoved;
                Window.Current.CoreWindow.PointerPressed += CoreWindowPointerChanged;
                Window.Current.CoreWindow.PointerReleased += CoreWindowPointerChanged;
                Window.Current.Activated += WindowActivated;
                ApplicationData.Current.DataChanged += PreferencesChanged;
                lastFitRequest = HudPreferences.FitRequest;
                ApplyPreferences();
                EnsureDefaultVoiceChatPosition();
                // Start large, then adapt once to this display. Never overwrite
                // a pinned, explicitly configured or previously initialized layout.
                if (!HudPreferences.InitialFitDone)
                {
                    bool adapt = HudPreferences.FitRequest == 0 && !widget.Pinned;
                    HudPreferences.MarkInitialFitDone();
                    if (adapt) await FitToDisplayAsync();
                }
                UpdateInputState();
                UpdatePinTutorial();
                QueueTutorialStateRefresh("loaded");
                timer.Start();
                await RefreshAsync();
                QueueTutorialStateRefresh("ready");
                App.LogStage("hud-ready");
            };
            Unloaded += (_, __) =>
            {
                App.LogStage("hud-unloaded");
                hudLoaded = false;
                Window.Current.CoreWindow.KeyDown -= CoreWindowKeyDown;
                Window.Current.CoreWindow.PointerMoved -= CoreWindowPointerMoved;
                Window.Current.CoreWindow.PointerPressed -= CoreWindowPointerChanged;
                Window.Current.CoreWindow.PointerReleased -= CoreWindowPointerChanged;
                Window.Current.Activated -= WindowActivated;
                ApplicationData.Current.DataChanged -= PreferencesChanged;
                // Keep the diagnostic loop alive across transient Game Bar unload/reload cycles.
                // The watchdog will recover it if the host pauses or drops dispatcher work.
                _ = SyncExternalClickTestAsync(false);
                Interlocked.Increment(ref tutorialRefreshGeneration);
                Interlocked.Increment(ref pinnedCoverageRestoreGeneration);
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
            var hudVisible = HudPreferences.Visible;
            HudPanel.Visibility = hudVisible ? Visibility.Visible : Visibility.Collapsed;
            HudPanel.Opacity = HudPreferences.Opacity;
            RootGrid.Background = hudVisible && HudPreferences.CoverageTint
                ? new SolidColorBrush(Color.FromArgb(26, 123, 90, 166))
                : new SolidColorBrush(Colors.Transparent);
            ApplyCursorMarkersVisibility(hudVisible && HudPreferences.NativeCursorMarkersVisible);
            ApplyVoiceChatPreferences();
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
            App.LogStage("hud-navigated-to");
            if (widget != null)
            {
                notificationManager = new XboxGameBarWidgetNotificationManager(widget);
                widget.SettingsClicked += WidgetSettingsClicked;
                widget.WindowBoundsChanged += WidgetStateChanged;
                widget.PinnedChanged += WidgetStateChanged;
                widget.ClickThroughEnabledChanged += WidgetStateChanged;
                widget.GameBarDisplayModeChanged += DisplayModeChanged;
                StartNativeInputLoop();
                _ = SyncExternalClickTestAsync(true);
                UpdatePinTutorial();
                QueueTutorialStateRefresh("navigated");
            }
            else
            {
                ClickNativeResult.Text = "Agent native input unavailable · widget=null";
            }
            base.OnNavigatedTo(e);
        }

        protected override void OnNavigatedFrom(NavigationEventArgs e)
        {
            App.LogStage("hud-navigated-from");
            if (widget != null)
            {
                widget.SettingsClicked -= WidgetSettingsClicked;
                widget.WindowBoundsChanged -= WidgetStateChanged;
                widget.PinnedChanged -= WidgetStateChanged;
                widget.ClickThroughEnabledChanged -= WidgetStateChanged;
                widget.GameBarDisplayModeChanged -= DisplayModeChanged;
            }
            notificationManager = null;
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
                UpdatePinTutorial();
                QueuePinnedCoverageRestore("widget-state");
            });
        }

        private async void DisplayModeChanged(XboxGameBarWidget sender, object args)
        {
            await Dispatcher.RunAsync(CoreDispatcherPriority.Normal, () =>
            {
                UpdateDisplayState();
                UpdateInputState();
                UpdatePinTutorial();
                // In full-size mode Game Bar's hover/title chrome may temporarily
                // reserve a strip of the host window. Restore coverage only after
                // the widget returns to pinned-only mode; compact/manual setup is
                // intentionally left untouched.
                QueuePinnedCoverageRestore("display-mode");
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

        private bool IsPinnedFullSizeMode()
        {
            return widget != null && hudLoaded && !HudPreferences.CompactLayout &&
                widget.Pinned && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly;
        }

        private bool PinnedCoverageNeedsRestore()
        {
            if (!IsPinnedFullSizeMode()) return false;
            UpdateDisplayMetrics();
            var bounds = widget.WindowBounds;
            var contentWidth = ActualWidth > 0 ? ActualWidth : bounds.Width;
            var contentHeight = ActualHeight > 0 ? ActualHeight : bounds.Height;
            const double tolerance = 2.0;

            // WindowBounds is the widget position on the display. In pinned-only
            // mode the content is expected to cover the display in full-size mode.
            // A stale title/chrome reservation shows up as a small positive bottom
            // or right gap (and often a non-zero Y).
            var rightGap = displayWidth - (bounds.X + contentWidth);
            var bottomGap = displayHeight - (bounds.Y + contentHeight);
            // Preserve the window position chosen by the user. A non-zero X/Y
            // is valid; only restore when the hosted content actually stops short
            // of the display's right or bottom edge.
            return rightGap > tolerance || bottomGap > tolerance;
        }

        private void QueuePinnedCoverageRestore(string reason)
        {
            if (!IsPinnedFullSizeMode()) return;
            var generation = Interlocked.Increment(ref pinnedCoverageRestoreGeneration);
            _ = RestorePinnedCoverageAfterChromeAsync(generation, reason);
        }

        private async Task RestorePinnedCoverageAfterChromeAsync(int generation, string reason)
        {
            // Host title/pin chrome can linger briefly after hover or Win+G closes.
            // Retry only for a short window and only while the full-size pinned mode
            // is still active. A later bounds event starts a fresh generation.
            var delays = new[] { 120, 320, 700 };
            foreach (var delayMs in delays)
            {
                await Task.Delay(delayMs);
                if (generation != Volatile.Read(ref pinnedCoverageRestoreGeneration)) return;
                if (!IsPinnedFullSizeMode()) return;
                if (!PinnedCoverageNeedsRestore()) return;
                if (pinnedCoverageRestoreInFlight) return;

                pinnedCoverageRestoreInFlight = true;
                try
                {
                    App.LogStage("pinned-coverage-restore reason=" + reason + " delayMs=" + delayMs +
                        " bounds=" + widget.WindowBounds + " content=" + ActualWidth.ToString("0.#") + "x" + ActualHeight.ToString("0.#"));
                    await FitToDisplayAsync(false);
                }
                finally
                {
                    pinnedCoverageRestoreInFlight = false;
                }

                if (!PinnedCoverageNeedsRestore())
                {
                    App.LogStage("pinned-coverage-restored reason=" + reason);
                    return;
                }
            }
        }

        private void QueueTutorialStateRefresh(string reason)
        {
            var generation = Interlocked.Increment(ref tutorialRefreshGeneration);
            _ = RefreshTutorialStateAfterHostSettlesAsync(generation, reason);
        }

        private async Task RefreshTutorialStateAfterHostSettlesAsync(int generation, string reason)
        {
            // Xbox Game Bar can expose a provisional PinnedOnly display mode while
            // the widget is first being activated. Re-check for a short bounded
            // window so the first Win+G does not depend on a later resize event.
            var delays = new[] { 50, 150, 350, 700 };
            foreach (var delayMs in delays)
            {
                await Task.Delay(delayMs);
                if (generation != Volatile.Read(ref tutorialRefreshGeneration)) return;
                if (widget == null || !hudLoaded) return;

                await Dispatcher.RunAsync(CoreDispatcherPriority.Normal, () =>
                {
                    if (generation != Volatile.Read(ref tutorialRefreshGeneration)) return;
                    UpdatePinTutorial();
                });

                if (widget != null && widget.GameBarDisplayMode != XboxGameBarDisplayMode.PinnedOnly)
                {
                    App.LogStage("tutorial-host-settled reason=" + reason + " delayMs=" + delayMs);
                    return;
                }
            }
        }

        private void UpdatePinTutorial()
        {
            if (PinTutorialOverlay == null || ControlsTutorialOverlay == null || widget == null) return;

            bool gameBarOpen = widget.GameBarDisplayMode != XboxGameBarDisplayMode.PinnedOnly;

            if (tutorialResetAwaitingUnpin && !widget.Pinned)
            {
                tutorialResetAwaitingUnpin = false;
                App.LogStage("tutorial-reset-unpinned");
            }

            if (widget.Pinned && !tutorialResetAwaitingUnpin && !HudPreferences.PinTutorialCompleted)
            {
                HudPreferences.MarkPinTutorialCompleted();
                App.LogStage("pin-tutorial-completed");
            }

            bool showPinTutorial = gameBarOpen && !widget.Pinned && !HudPreferences.PinTutorialCompleted;
            PinTutorialOverlay.Visibility = showPinTutorial ? Visibility.Visible : Visibility.Collapsed;
            if (showPinTutorial)
            {
                ControlsTutorialOverlay.Visibility = Visibility.Collapsed;
                App.LogStage("pin-tutorial-visible");
                return;
            }

            if (!gameBarOpen)
            {
                PinTutorialOverlay.Visibility = Visibility.Collapsed;
                ControlsTutorialOverlay.Visibility = Visibility.Collapsed;
                if (controlsTutorialShownThisSession && !HudPreferences.ControlsTutorialCompleted)
                {
                    HudPreferences.MarkControlsTutorialCompleted();
                    controlsTutorialShownThisSession = false;
                    App.LogStage("controls-tutorial-completed");
                }
                return;
            }

            bool showControlsTutorial = HudPreferences.PinTutorialCompleted && !HudPreferences.ControlsTutorialCompleted;
            ControlsTutorialOverlay.Visibility = showControlsTutorial ? Visibility.Visible : Visibility.Collapsed;
            if (showControlsTutorial && !controlsTutorialShownThisSession)
            {
                controlsTutorialShownThisSession = true;
                App.LogStage("controls-tutorial-visible");
            }
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
                lastSnapshotSuccessAtMs = NowMs();
                consecutiveSnapshotFailures = 0;
            }
            catch (Exception ex) { HandleSnapshotFailure(ex); }
            finally { refreshing = false; }
        }

        private void HandleSnapshotFailure(Exception error)
        {
            var now = NowMs();
            consecutiveSnapshotFailures++;
            var recentSuccess = lastSnapshotSuccessAtMs > 0 && now - lastSnapshotSuccessAtMs <= 10000;

            // A single localhost timeout does not mean the desktop Agent died.
            // Keep the last good snapshot visible while the 1s poll retries.
            if (recentSuccess && consecutiveSnapshotFailures <= 2)
            {
                bridgeStatus = "delayed";
                bridgeError = ErrorCode(error);
                BridgeText.Text = "에이전트 응답 지연 · 자동 재시도 중";
                if (now - lastSnapshotFailureLogAtMs >= 5000)
                {
                    lastSnapshotFailureLogAtMs = now;
                    App.LogStage("snapshot-transient-failure count=" + consecutiveSnapshotFailures + " error=" + bridgeError);
                }
                SaveStatus();
                return;
            }

            SetOffline(error);
        }

        private async Task<string> ReadLoopbackSnapshotAsync()
        {
            using (var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(3)))
            using (var socket = new StreamSocket())
            {
                await socket.ConnectAsync(new HostName("127.0.0.1"), "19514").AsTask(timeout.Token);
                using (var stream = socket.InputStream.AsStreamForRead())
                    return await NativePipeClient.ReadLineAsync(stream, timeout.Token);
            }
        }

        private void Apply(JsonObject json)
        {
            ApplyTutorialResetRequest(json);
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
            ApplyVoiceChat(json);
            SaveStatus();
        }

        private void ApplyTutorialResetRequest(JsonObject json)
        {
            long generation = 0;
            try
            {
                if (json.ContainsKey("tutorial_reset_generation") &&
                    json["tutorial_reset_generation"].ValueType == JsonValueType.Number)
                {
                    generation = (long)json.GetNamedNumber("tutorial_reset_generation", 0);
                }
            }
            catch { generation = 0; }

            if (generation <= 0 || generation == lastTutorialResetGeneration) return;
            lastTutorialResetGeneration = generation;
            HudPreferences.ResetTutorials();
            controlsTutorialShownThisSession = false;
            tutorialResetAwaitingUnpin = widget != null && widget.Pinned;
            UpdatePinTutorial();
            App.LogStage("tutorial-reset generation=" + generation);
        }

        private void ApplyVoiceChat(JsonObject json)
        {
            var now = NowMs();
            JsonArray messages = null;
            if (json.ContainsKey("voice_chat_messages") && json["voice_chat_messages"].ValueType == JsonValueType.Array)
                messages = json.GetNamedArray("voice_chat_messages");

            if (messages == null || messages.Count == 0)
            {
                voiceChatFirstSeenAtMs.Clear();
                voiceChatSignature = "";
                voiceChatHasVisibleMessages = false;
                VoiceChatOverlay.Children.Clear();
                voiceChatRenderedKeys.Clear();
                UpdateVoiceChatPanelChrome(widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly);
                return;
            }

            var lifetimeMs = (long)Math.Round(HudPreferences.VoiceChatMessageLifetimeSeconds * 1000.0);
            var snapshotKeys = new HashSet<string>(StringComparer.Ordinal);
            var visible = new List<Tuple<string, JsonObject>>();
            foreach (var value in messages)
            {
                if (value.ValueType != JsonValueType.Object) continue;
                var item = value.GetObject();
                var content = SnapshotJson.String(item, "content");
                if (string.IsNullOrWhiteSpace(content)) continue;
                var key = VoiceChatMessageKey(item);
                snapshotKeys.Add(key);
                long firstSeen;
                if (!voiceChatFirstSeenAtMs.TryGetValue(key, out firstSeen))
                {
                    firstSeen = now;
                    voiceChatFirstSeenAtMs[key] = firstSeen;
                }
                if (now - firstSeen < lifetimeMs) visible.Add(Tuple.Create(key, item));
            }

            foreach (var key in voiceChatFirstSeenAtMs.Keys.Where(key => !snapshotKeys.Contains(key)).ToList())
                voiceChatFirstSeenAtMs.Remove(key);

            var signature = string.Join("|", visible.Select(item => item.Item1));
            voiceChatHasVisibleMessages = visible.Count > 0;
            if (signature == voiceChatSignature)
            {
                UpdateVoiceChatPanelChrome(widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly);
                return;
            }
            voiceChatSignature = signature;
            var previouslyRendered = new HashSet<string>(voiceChatRenderedKeys, StringComparer.Ordinal);
            voiceChatRenderedKeys.Clear();
            VoiceChatOverlay.Children.Clear();
            var addedNewMessage = false;

            foreach (var entry in visible.Skip(Math.Max(0, visible.Count - 6)))
            {
                var key = entry.Item1;
                var item = entry.Item2;
                var author = SnapshotJson.String(item, "author_name");
                var content = SnapshotJson.String(item, "content");
                var avatar = SnapshotJson.String(item, "avatar_url");

                var row = new Grid { Margin = new Thickness(0, 0, 0, 2) };
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(34) });
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

                var avatarCircle = new Ellipse
                {
                    Width = 28,
                    Height = 28,
                    VerticalAlignment = VerticalAlignment.Top,
                    Margin = new Thickness(0, 1, 6, 0),
                    Fill = new SolidColorBrush(Color.FromArgb(255, 54, 57, 63)),
                };
                if (Uri.TryCreate(avatar, UriKind.Absolute, out var avatarUri) && avatarUri.Scheme == Uri.UriSchemeHttps)
                {
                    BitmapImage avatarImage;
                    if (!voiceChatAvatarCache.TryGetValue(avatar, out avatarImage))
                    {
                        avatarImage = new BitmapImage
                        {
                            DecodePixelWidth = 56,
                            UriSource = avatarUri,
                        };
                        if (voiceChatAvatarCache.Count >= 32) voiceChatAvatarCache.Clear();
                        voiceChatAvatarCache[avatar] = avatarImage;
                    }
                    avatarCircle.Fill = new ImageBrush
                    {
                        ImageSource = avatarImage,
                        Stretch = Stretch.UniformToFill,
                    };
                }
                Grid.SetColumn(avatarCircle, 0);
                row.Children.Add(avatarCircle);

                var copy = new StackPanel { Spacing = 0 };
                copy.Children.Add(new TextBlock
                {
                    Text = author,
                    FontSize = 11,
                    FontWeight = Windows.UI.Text.FontWeights.SemiBold,
                    Foreground = new SolidColorBrush(Color.FromArgb(255, 222, 214, 238)),
                    TextTrimming = TextTrimming.CharacterEllipsis,
                });
                copy.Children.Add(new TextBlock
                {
                    Text = content,
                    FontSize = 13,
                    FontWeight = Windows.UI.Text.FontWeights.SemiBold,
                    Foreground = new SolidColorBrush(Colors.White),
                    TextWrapping = TextWrapping.WrapWholeWords,
                    MaxWidth = 450,
                });
                Grid.SetColumn(copy, 1);
                row.Children.Add(copy);
                VoiceChatOverlay.Children.Add(row);
                voiceChatRenderedKeys.Add(key);
                if (!previouslyRendered.Contains(key))
                {
                    addedNewMessage = true;
                    AnimateVoiceChatEntry(row);
                }
            }

            if (addedNewMessage) AnimateVoiceChatStack();
            voiceChatHasVisibleMessages = VoiceChatOverlay.Children.Count > 0;
            UpdateVoiceChatPanelChrome(widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly);
        }


        private static void AnimateVoiceChatEntry(FrameworkElement row)
        {
            var translate = new TranslateTransform { Y = 8 };
            row.RenderTransform = translate;
            row.Opacity = 0;

            var easing = new CubicEase { EasingMode = EasingMode.EaseOut };
            var storyboard = new Storyboard();
            var slide = new DoubleAnimation
            {
                From = 8,
                To = 0,
                Duration = TimeSpan.FromMilliseconds(190),
                EasingFunction = easing,
                EnableDependentAnimation = true,
            };
            Storyboard.SetTarget(slide, translate);
            Storyboard.SetTargetProperty(slide, "Y");

            var fade = new DoubleAnimation
            {
                From = 0,
                To = 1,
                Duration = TimeSpan.FromMilliseconds(150),
                EasingFunction = easing,
            };
            Storyboard.SetTarget(fade, row);
            Storyboard.SetTargetProperty(fade, "Opacity");

            storyboard.Children.Add(slide);
            storyboard.Children.Add(fade);
            storyboard.Begin();
        }

        private void AnimateVoiceChatStack()
        {
            var translate = VoiceChatOverlay.RenderTransform as TranslateTransform;
            if (translate == null)
            {
                translate = new TranslateTransform();
                VoiceChatOverlay.RenderTransform = translate;
            }
            translate.Y = 4;

            var slide = new DoubleAnimation
            {
                From = 4,
                To = 0,
                Duration = TimeSpan.FromMilliseconds(180),
                EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut },
                EnableDependentAnimation = true,
            };
            Storyboard.SetTarget(slide, translate);
            Storyboard.SetTargetProperty(slide, "Y");
            var storyboard = new Storyboard();
            storyboard.Children.Add(slide);
            storyboard.Begin();
        }

        private static string VoiceChatMessageKey(JsonObject item)
        {
            return item.GetNamedNumber("timestamp_ms", 0).ToString("0", System.Globalization.CultureInfo.InvariantCulture) + ":" +
                SnapshotJson.String(item, "author_name") + ":" + SnapshotJson.String(item, "content");
        }

        private void ApplyVoiceChatPreferences()
        {
            ApplyVoiceChatBodyChrome(widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly);
            voiceChatSignature = "";
        }

        private void ApplyVoiceChatBodyChrome(bool pinnedOnly)
        {
            var baseColor = ParseHexColor(HudPreferences.VoiceChatBackgroundColor);
            var configuredAlpha = (byte)Math.Round(255.0 * HudPreferences.VoiceChatBackgroundOpacity);
            if (pinnedOnly)
            {
                // In the real pinned overlay there is no editor box. Preserve the
                // user's optional background setting; its default alpha is zero.
                VoiceChatBody.Background = new SolidColorBrush(Color.FromArgb(
                    configuredAlpha, baseColor.R, baseColor.G, baseColor.B));
                VoiceChatBody.BorderBrush = new SolidColorBrush(Colors.Transparent);
                VoiceChatBody.BorderThickness = new Thickness(0);
                return;
            }

            // Win+G is layout/edit mode: always expose the exact viewport so the
            // user can see where Discord messages are allowed to render.
            var editorAlpha = Math.Max(configuredAlpha, (byte)128);
            VoiceChatBody.Background = new SolidColorBrush(Color.FromArgb(
                (byte)editorAlpha, baseColor.R, baseColor.G, baseColor.B));
            VoiceChatBody.BorderBrush = new SolidColorBrush(Color.FromArgb(150, 123, 90, 166));
            VoiceChatBody.BorderThickness = new Thickness(1);
        }

        private static Color ParseHexColor(string value)
        {
            var text = (value ?? "").Trim();
            if (text.Length == 7 && text[0] == '#')
            {
                byte r, g, b;
                if (byte.TryParse(text.Substring(1, 2), System.Globalization.NumberStyles.HexNumber, null, out r) &&
                    byte.TryParse(text.Substring(3, 2), System.Globalization.NumberStyles.HexNumber, null, out g) &&
                    byte.TryParse(text.Substring(5, 2), System.Globalization.NumberStyles.HexNumber, null, out b))
                    return Color.FromArgb(255, r, g, b);
            }
            return Color.FromArgb(255, 20, 16, 32);
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
            var values = ApplicationData.Current.LocalSettings.Values;
            if (values.ContainsKey(VoiceChatPanelXSetting) || values.ContainsKey(VoiceChatPanelYSetting))
                RestorePanelPosition(VoiceChatPanel, VoiceChatPanelTransform, VoiceChatPanelXSetting, VoiceChatPanelYSetting);
            else
                voiceChatPositionNeedsDefault = true;
        }

        private void RestorePanelSizes()
        {
            var values = ApplicationData.Current.LocalSettings.Values;
            RestorePanelSize("lcu", values, LcuPanelWidthSetting, LcuPanelHeightSetting, 240, 145);
            RestorePanelSize("party", values, PartyPanelWidthSetting, PartyPanelHeightSetting, 240, 70);
            RestorePanelSize("youtube", values, YoutubePanelWidthSetting, YoutubePanelHeightSetting, 340, 191);
            RestorePanelSize("click-native", values, ClickNativePanelWidthSetting, ClickNativePanelHeightSetting, 250, 300);
            clickNativeExpandedHeight = Math.Max(120, ClickNativeBody.Height);
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
            else if (key == "click-native") { values[ClickNativePanelWidthSetting] = panel.Width; values[ClickNativePanelHeightSetting] = GetPanelExpanded(key) ? body.Height : clickNativeExpandedHeight; }
        }

        private FrameworkElement GetPanel(string key)
        {
            if (key == "lcu") return LcuPanel;
            if (key == "party") return PartyPanel;
            if (key == "youtube") return YoutubePanel;
            if (key == "click-native") return ClickNativePanel;
            if (key == "voice-chat") return VoiceChatPanel;
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
            if (key == "click-native") return HudPreferences.NativeInputPanelVisible;
            return false;
        }

        private void SetPanelVisible(string key, bool visible)
        {
            if (key == "lcu") HudPreferences.LcuPanelVisible = visible;
            else if (key == "party") HudPreferences.PartyPanelVisible = visible;
            else if (key == "youtube") YoutubePreferences.Visible = visible;
            else if (key == "click-native") HudPreferences.NativeInputPanelVisible = visible;
        }

        private bool GetPanelExpanded(string key)
        {
            if (key == "lcu") return HudPreferences.LcuPanelExpanded;
            if (key == "party") return HudPreferences.PartyPanelExpanded;
            if (key == "youtube") return HudPreferences.YoutubePanelExpanded;
            if (key == "click-native") return HudPreferences.NativeInputPanelExpanded;
            return true;
        }

        private void SetPanelExpanded(string key, bool expanded)
        {
            if (key == "lcu") HudPreferences.LcuPanelExpanded = expanded;
            else if (key == "party") HudPreferences.PartyPanelExpanded = expanded;
            else if (key == "youtube") HudPreferences.YoutubePanelExpanded = expanded;
            else if (key == "click-native") HudPreferences.NativeInputPanelExpanded = expanded;
        }

        private void UpdatePanelChrome()
        {
            bool pinnedOnly = widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly;
            ApplyPinnedBodyPositionOffset(pinnedOnly);
            ApplyPanelChrome("lcu", LcuPanel, LcuMoveBar, LcuBody, LcuEyeButton, LcuResizeHandles, pinnedOnly);
            ApplyPanelChrome("party", PartyPanel, PartyMoveBar, PartyBody, PartyEyeButton, PartyResizeHandles, pinnedOnly);
            ApplyPanelChrome("youtube", YoutubePanel, YoutubeMoveBar, YoutubeVideoHost, YoutubeEyeButton, YoutubeResizeHandles, pinnedOnly);
            ApplyClickTestPanelChrome(pinnedOnly);
            UpdateVoiceChatPanelChrome(pinnedOnly);
        }

        private void ApplyClickTestPanelChrome(bool pinnedOnly)
        {
            var visible = HudPreferences.NativeInputPanelVisible;
            var expanded = HudPreferences.NativeInputPanelExpanded;
            ClickNativePanel.Visibility = visible ? Visibility.Visible : Visibility.Collapsed;
            ClickNativeMoveBar.Visibility = visible && !pinnedOnly ? Visibility.Visible : Visibility.Collapsed;
            ClickNativeBody.Visibility = visible ? Visibility.Visible : Visibility.Collapsed;
            ClickNativeDetails.Visibility = expanded ? Visibility.Visible : Visibility.Collapsed;
            ClickNativeEyeButton.Opacity = expanded ? 1.0 : 0.5;
            ToolTipService.SetToolTip(ClickNativeEyeButton, expanded ? "상세 내용 가리기" : "상세 내용 표시");
            ClickNativeResizeHandles.Visibility = visible && expanded && !pinnedOnly ? Visibility.Visible : Visibility.Collapsed;

            if (expanded)
            {
                if (ClickNativeBody.Height <= 70) ClickNativeBody.Height = Math.Max(120, clickNativeExpandedHeight);
            }
            else
            {
                if (ClickNativeBody.Height > 70) clickNativeExpandedHeight = ClickNativeBody.Height;
                ClickNativeBody.Height = 70;
            }
        }

        private void UpdateVoiceChatPanelChrome(bool pinnedOnly)
        {
            var editing = !pinnedOnly;
            VoiceChatPanel.Visibility = editing || voiceChatHasVisibleMessages ? Visibility.Visible : Visibility.Collapsed;
            VoiceChatMoveBar.Visibility = editing ? Visibility.Visible : Visibility.Collapsed;
            VoiceChatBody.Visibility = editing || voiceChatHasVisibleMessages ? Visibility.Visible : Visibility.Collapsed;
            VoiceChatOverlay.Visibility = voiceChatHasVisibleMessages ? Visibility.Visible : Visibility.Collapsed;
            VoiceChatPlaceholder.Visibility = editing && !voiceChatHasVisibleMessages ? Visibility.Visible : Visibility.Collapsed;
            ApplyVoiceChatBodyChrome(pinnedOnly);
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
            VoiceChatPanelTransform.Y += delta;
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
            else if (panel == VoiceChatPanel) { values[VoiceChatPanelXSetting] = transform.X; values[VoiceChatPanelYSetting] = transform.Y; }
        }

        private void ClampAllPanelPositions()
        {
            SetPanelPosition(LcuPanel, LcuPanelTransform, LcuPanelTransform.X, LcuPanelTransform.Y, false);
            SetPanelPosition(PartyPanel, PartyPanelTransform, PartyPanelTransform.X, PartyPanelTransform.Y, false);
            SetPanelPosition(YoutubePanel, YoutubePanelTransform, YoutubePanelTransform.X, YoutubePanelTransform.Y, false);
            SetPanelPosition(ClickNativePanel, ClickNativePanelTransform, ClickNativePanelTransform.X, ClickNativePanelTransform.Y, false);
            SetPanelPosition(VoiceChatPanel, VoiceChatPanelTransform, VoiceChatPanelTransform.X, VoiceChatPanelTransform.Y, false);
        }

        private void EnsureDefaultVoiceChatPosition()
        {
            if (!voiceChatPositionNeedsDefault || RootGrid.ActualHeight <= 0) return;
            var panelHeight = VoiceChatPanel.ActualHeight > 0 ? VoiceChatPanel.ActualHeight : 150.0;
            var y = Math.Max(0.0, RootGrid.ActualHeight - VoiceChatPanel.Margin.Top - panelHeight - 72.0);
            SetPanelPosition(VoiceChatPanel, VoiceChatPanelTransform, 0.0, y, false);
            voiceChatPositionNeedsDefault = false;
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


        private long NowMs() => DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();

        private void StartNativeInputLoop()
        {
            if (nativeInputLoopCts != null && !nativeInputLoopCts.IsCancellationRequested) return;
            nativeInputLoopCts = new CancellationTokenSource();
            var token = nativeInputLoopCts.Token;
            App.LogStage("native-input-loop-start");
            _ = Task.Run(async () =>
            {
                while (!token.IsCancellationRequested)
                {
                    var dispatchAge = nativeInputDispatchQueuedAtMs <= 0 ? 0 : NowMs() - nativeInputDispatchQueuedAtMs;
                    if (Volatile.Read(ref nativeInputDispatchInFlight) != 0 && dispatchAge >= 500)
                    {
                        Interlocked.Exchange(ref nativeInputDispatchInFlight, 0);
                        nativeInputDispatchQueuedAtMs = 0;
                        App.LogStage("native-input-dispatch-reset ageMs=" + dispatchAge);
                    }
                    if (Interlocked.Exchange(ref nativeInputDispatchInFlight, 1) == 0)
                    {
                        nativeInputDispatchQueuedAtMs = NowMs();
                        try
                        {
                            // Keep at most one UI probe queued. If Game Bar starves or
                            // invalidates its dispatcher during a host state transition,
                            // piling up callbacks makes recovery worse when it resumes.
                            _ = Dispatcher.RunAsync(CoreDispatcherPriority.Low, async () =>
                            {
                                try { await SyncExternalClickTestAsync(true); }
                                catch (Exception ex) { App.LogStage("native-input-loop-sync-error:" + ErrorCode(ex)); }
                                finally { Interlocked.Exchange(ref nativeInputDispatchInFlight, 0); nativeInputDispatchQueuedAtMs = 0; }
                            });
                        }
                        catch (Exception ex)
                        {
                            Interlocked.Exchange(ref nativeInputDispatchInFlight, 0);
                            nativeInputDispatchQueuedAtMs = 0;
                            App.LogStage("native-input-loop-dispatch-error:" + ErrorCode(ex));
                        }
                    }

                    var age = lastNativeInputSyncOkAtMs <= 0 ? -1 : NowMs() - lastNativeInputSyncOkAtMs;
                    if (age > 500)
                    {
                        // The Game Bar UI dispatcher can become temporarily invalid or
                        // starved across Alt+Tab/host state changes. Keep the Agent-side
                        // native hitbox alive without depending on any UI/COM objects.
                        _ = TryNativeInputKeepaliveAsync();
                        var now = NowMs();
                        if (now - lastNativeInputStaleLogAtMs >= 1000)
                        {
                            lastNativeInputStaleLogAtMs = now;
                            App.LogStage("native-input-loop-stale ageMs=" + age);
                        }
                    }

                    try { await Task.Delay(100, token); }
                    catch (TaskCanceledException) { break; }
                }
                App.LogStage("native-input-loop-stop");
            }, token);
        }

        private void StopNativeInputLoop()
        {
            try { nativeInputLoopCts?.Cancel(); } catch { }
            try { nativeInputLoopCts?.Dispose(); } catch { }
            nativeInputLoopCts = null;
        }

        private async Task TryNativeInputKeepaliveAsync()
        {
            var payload = lastNativeInputKeepalivePayload;
            if (string.IsNullOrWhiteSpace(payload)) return;
            if (Interlocked.Exchange(ref nativeInputKeepaliveInFlight, 1) != 0) return;
            try
            {
                var rawResponse = await SendAgentCommandAsync(payload).ConfigureAwait(false);
                var response = JsonObject.Parse(rawResponse);
                if (response.GetNamedBoolean("ok", false))
                {
                    lastNativeInputSyncOkAtMs = NowMs();
                }
            }
            catch (Exception ex)
            {
                var now = NowMs();
                if (now - lastNativeInputStaleLogAtMs >= 1000)
                {
                    lastNativeInputStaleLogAtMs = now;
                    App.LogStage("native-input-loop-keepalive-error:" + ErrorCode(ex));
                }
            }
            finally
            {
                Interlocked.Exchange(ref nativeInputKeepaliveInFlight, 0);
            }
        }

        private void WindowActivated(object sender, WindowActivatedEventArgs e)
        {
            App.LogStage("hud-window-activated-" + e.WindowActivationState);
            // A queued callback can belong to an invalidated Game Bar dispatcher.
            // Allow a fresh probe as soon as the host/window becomes active again.
            Interlocked.Exchange(ref nativeInputDispatchInFlight, 0);
            nativeInputDispatchQueuedAtMs = 0;
            StartNativeInputLoop();
            _ = SyncExternalClickTestAsync(true);
            UpdatePinTutorial();
            QueueTutorialStateRefresh("window-activated-" + e.WindowActivationState);
            QueuePinnedCoverageRestore("window-activated-" + e.WindowActivationState);
        }

        private async Task SyncExternalClickTestAsync(bool active)
        {
            var now = NowMs();
            if (syncingNativeInput)
            {
                // A single UI/COM call can occasionally never complete. Do not let
                // that permanently disable every later native-input sync.
                var inFlightAge = syncingNativeInputSinceMs <= 0 ? 0 : now - syncingNativeInputSinceMs;
                if (inFlightAge < 500) return;
                App.LogStage("native-input-sync-reset ageMs=" + inFlightAge);
                Interlocked.Increment(ref nativeInputSyncGeneration);
                syncingNativeInput = false;
                syncingNativeInputSinceMs = 0;
            }
            if (widget == null)
            {
                ClickNativeResult.Text = "Agent native input unavailable · widget=null";
                return;
            }
            syncingNativeInput = true;
            syncingNativeInputSinceMs = NowMs();
            var syncGeneration = Interlocked.Increment(ref nativeInputSyncGeneration);
            try
            {
                var requestedActive = active && HudPreferences.Visible && HudPanel.Visibility == Visibility.Visible
                    && ClickNativePanel.Visibility == Visibility.Visible;
                var geometryReady = NativeHookHitbox.ActualWidth > 0 && NativeHookHitbox.ActualHeight > 0;
                var effectiveActive = requestedActive && geometryReady;
                // Do not depend on PointerMoved events staying alive after a click.
                // CoreWindow.PointerPosition can be polled directly and returns the
                // current client-space pointer position in DIPs.
                try
                {
                    UpdateGameBarPointer(Window.Current.CoreWindow.PointerPosition);
                }
                catch { }

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

                var payload = command.Stringify();
                lastNativeInputKeepalivePayload = payload;
                var rawResponse = await SendAgentCommandAsync(payload);
                lastNativeInputSyncOkAtMs = NowMs();
                var response = JsonObject.Parse(rawResponse);
                if (!response.GetNamedBoolean("ok", false))
                {
                    ClickNativeResult.Text = "Agent native · " + response.GetNamedString("message", "sync failed");
                    return;
                }
                var sequence = (ulong)Math.Max(0, response.GetNamedNumber("sequence", 0));
                var clickCount = (ulong)Math.Max(0, response.GetNamedNumber("clickCount", 0));
                var hudToggleSequence = (ulong)Math.Max(0, response.GetNamedNumber("hudToggleSequence", 0));
                if (!hudToggleSequenceInitialized)
                {
                    lastHudToggleSequence = hudToggleSequence;
                    hudToggleSequenceInitialized = true;
                }
                else if (hudToggleSequence != lastHudToggleSequence)
                {
                    lastHudToggleSequence = hudToggleSequence;
                    HudPreferences.Visible = !HudPreferences.Visible;
                    ApplyPreferences();
                    App.LogStage("hud-toggle-f9-visible=" + HudPreferences.Visible);
                }
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
                var physicalX = (int)response.GetNamedNumber("physicalX", 0);
                var physicalY = (int)response.GetNamedNumber("physicalY", 0);
                var logicalX = (int)response.GetNamedNumber("logicalX", 0);
                var logicalY = (int)response.GetNamedNumber("logicalY", 0);
                var cursorInfoX = (int)response.GetNamedNumber("cursorInfoX", 0);
                var cursorInfoY = (int)response.GetNamedNumber("cursorInfoY", 0);
                var cursorInfoValid = response.GetNamedBoolean("cursorInfoValid", false);
                var cursorInfoFlags = (uint)Math.Max(0, response.GetNamedNumber("cursorInfoFlags", 0));
                var cursorInfoShowing = response.GetNamedBoolean("cursorInfoShowing", false);
                var cursorInfoHandle = response.GetNamedString("cursorInfoHandle", "0x0");
                var gamebarPrimaryX = (int)Math.Round(response.GetNamedNumber("gamebarPrimaryX", 0));
                var gamebarPrimaryY = (int)Math.Round(response.GetNamedNumber("gamebarPrimaryY", 0));
                var gamebarAltX = (int)Math.Round(response.GetNamedNumber("gamebarAltX", 0));
                var gamebarAltY = (int)Math.Round(response.GetNamedNumber("gamebarAltY", 0));
                var gamebarStoredX = (int)response.GetNamedNumber("gamebarStoredCursorX", 0);
                var gamebarStoredY = (int)response.GetNamedNumber("gamebarStoredCursorY", 0);
                var gamebarAgeMs = (ulong)Math.Max(0, response.GetNamedNumber("gamebarCursorAgeMs", 0));
                if (sequence != lastNativeInputSequence) lastNativeInputSequence = sequence;

                UpdateCursorDiagnosticMarkers(
                    physicalX, physicalY, logicalX, logicalY, cursorInfoX, cursorInfoY, cursorInfoValid,
                    gamebarPrimaryX, gamebarPrimaryY, gamebarAltX, gamebarAltY, cursorX, cursorY);

                ClickNativeResult.Text =
                    "transport=" + lastAgentCommandTransport + " pipeErr=" + lastAgentPipeError + " tcpErr=" + lastAgentTcpError +
                    " requestedActive=" + requestedActive + " geometryReady=" + geometryReady +
                    " hitbox=" + NativeHookHitbox.ActualWidth.ToString("0.##") + "x" + NativeHookHitbox.ActualHeight.ToString("0.##") + "\n" +
                    "agent pid=" + agentPid + " build=" + agentBuild + " release=" + releaseLabel + " commit=" + agentCommit + "\n" +
                    "active=" + responseActive + " seq=" + sequence + " clicks=" + clickCount + " event=" + eventName +
                    " eventXY=" + eventX + "," + eventY + " eventAt=" + eventAtMs + " matched=" + matched + "\n" +
                    "physical(GetPhysicalCursorPos) = " + physicalX + "," + physicalY + "\n" +
                    "logical(GetCursorPos)        = " + logicalX + "," + logicalY + "\n" +
                    "cursorInfo(GetCursorInfo)    = " + cursorInfoX + "," + cursorInfoY + " valid=" + cursorInfoValid +
                    " flags=0x" + cursorInfoFlags.ToString("X") + " showing=" + cursorInfoShowing + " hCursor=" + cursorInfoHandle + "\n" +
                    "gamebar-primary              = " + gamebarPrimaryX + "," + gamebarPrimaryY + "\n" +
                    "gamebar-alt                  = " + gamebarAltX + "," + gamebarAltY + "\n" +
                    "gamebar-stored               = " + gamebarStoredX + "," + gamebarStoredY + " ageMs=" + gamebarAgeMs + "\n" +
                    "selected                      = " + cursorX + "," + cursorY + " source=" + cursorSource + "\n" +
                    "monitor-origin                = " + monitorX + "," + monitorY + " pollCount=" + pollCount + " pollAgeMs=" + pollAgeMs + "\n" +
                    "hit-test event                = " + eventX + "," + eventY + " matched=" + matched + "\n" +
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
                // Ignore completion from a sync that was already abandoned by the
                // watchdog; it must not unlock a newer in-flight operation.
                if (syncGeneration == Volatile.Read(ref nativeInputSyncGeneration))
                {
                    syncingNativeInput = false;
                    syncingNativeInputSinceMs = 0;
                }
            }
        }

        private void CursorMarkersToggle_Toggled(object sender, RoutedEventArgs e)
        {
            var visible = CursorMarkersToggle != null && CursorMarkersToggle.IsOn;
            HudPreferences.NativeCursorMarkersVisible = visible;
            ApplyCursorMarkersVisibility(visible);
        }

        private void ApplyCursorMarkersVisibility(bool visible)
        {
            if (NativeCursorOverlay != null)
                NativeCursorOverlay.Visibility = visible ? Visibility.Visible : Visibility.Collapsed;
        }

        private void UpdateCursorDiagnosticMarkers(
            int physicalX, int physicalY, int logicalX, int logicalY,
            int cursorInfoX, int cursorInfoY, bool cursorInfoValid,
            int gamebarPrimaryX, int gamebarPrimaryY, int gamebarAltX, int gamebarAltY,
            int selectedX, int selectedY)
        {
            if (widget == null || !HudPreferences.Visible || !HudPreferences.NativeCursorMarkersVisible) return;
            NativeCursorOverlay.Visibility = Visibility.Visible;
            var bounds = widget.WindowBounds;
            var scale = Math.Max(1.0, DisplayInformation.GetForCurrentView().RawPixelsPerViewPixel);

            void Place(FrameworkElement marker, double screenX, double screenY, bool visible = true)
            {
                if (!visible)
                {
                    marker.Visibility = Visibility.Collapsed;
                    return;
                }
                // Agent/native coordinates are physical pixels. Convert them back to
                // the widget's DIP coordinate space so each source can be compared
                // directly against the real cursor.
                var localX = (screenX / scale) - bounds.X;
                var localY = (screenY / scale) - bounds.Y;
                Canvas.SetLeft(marker, localX - (marker.ActualWidth > 0 ? marker.ActualWidth / 2.0 : 7.0));
                Canvas.SetTop(marker, localY - (marker.ActualHeight > 0 ? marker.ActualHeight / 2.0 : 7.0));
                marker.Visibility = Visibility.Visible;
            }

            Place(CursorMarkerPhysical, physicalX, physicalY);
            Place(CursorMarkerLogical, logicalX, logicalY);
            Place(CursorMarkerInfo, cursorInfoX, cursorInfoY, cursorInfoValid);
            Place(CursorMarkerGameBarPrimary, gamebarPrimaryX, gamebarPrimaryY);
            Place(CursorMarkerGameBarAlt, gamebarAltX, gamebarAltY);
            Place(CursorMarkerSelected, selectedX, selectedY);
        }

        private async Task<string> SendAgentCommandAsync(string payload)
        {
            // Desktop Agent <-> UWP Game Bar uses loopback TCP as the canonical
            // transport. Named pipes are intentionally skipped because UWP named
            // pipes are restricted to the same AppContainer app.
            lastAgentPipeError = "";
            try
            {
                using (var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(2)))
                using (var socket = new StreamSocket())
                {
                    await socket.ConnectAsync(new HostName("127.0.0.1"), "19515").AsTask(timeout.Token);
                    using (var output = socket.OutputStream.AsStreamForWrite())
                    using (var input = socket.InputStream.AsStreamForRead())
                    {
                        var data = Encoding.UTF8.GetBytes(payload + "\n");
                        await output.WriteAsync(data, 0, data.Length, timeout.Token).ConfigureAwait(false);
                        await output.FlushAsync(timeout.Token).ConfigureAwait(false);
                        var response = await NativePipeClient.ReadLineAsync(input, timeout.Token).ConfigureAwait(false);
                        lastAgentCommandTransport = "tcp:19515";
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

        private async void YoutubeSearchButton_Click(object sender, RoutedEventArgs e)
        {
            if (youtubeBrowseMode)
            {
                await CloseYoutubeBrowserAsync();
                return;
            }
            await OpenYoutubeBrowserAsync();
        }

        private async void YoutubeNotificationTestButton_Click(object sender, RoutedEventArgs e)
        {
            if (widget == null)
            {
                App.LogStage("notification-test-widget-null");
                return;
            }
            try
            {
                if (notificationManager == null)
                    notificationManager = new XboxGameBarWidgetNotificationManager(widget);
                var notification = new XboxGameBarWidgetNotificationBuilder("유미지지 오버레이 알림 테스트")
                    .Content("Game Bar 알림이 정상적으로 동작하고 있습니다.")
                    .BuildNotification();
                var result = await notificationManager.TryShowAsync(notification);
                App.LogStage("notification-test-result-" + result);
            }
            catch (Exception ex)
            {
                App.LogStage("notification-test-error-" + ErrorCode(ex));
            }
        }

        private async Task OpenYoutubeBrowserAsync()
        {
            youtubeBrowseMode = true;
            youtubeReady = false;
            youtubePlaying = false;
            YoutubeSearchIcon.Text = "\uE72B";
            ToolTipService.SetToolTip(YoutubeSearchButton, "플레이어로 돌아가기");
            youtubePlayerHeightBeforeBrowse = YoutubeVideoHost.Height;
            if (YoutubeVideoHost.Height < 360) YoutubeVideoHost.Height = 360;
            SetYoutubeInlineState(true, "YouTube를 여는 중…");

            const string youtubeHome = "https://www.youtube.com/";
            if (await EnsureYoutubeWebView2Async())
            {
                youtubeUsingFallback = false;
                YoutubeFallbackWebView.Visibility = Visibility.Collapsed;
                youtubeWebView2.Visibility = Visibility.Visible;
                youtubeWebView2.Source = new Uri(youtubeHome);
                return;
            }

            youtubeUsingFallback = true;
            YoutubeFallbackWebView.Visibility = Visibility.Visible;
            YoutubeFallbackWebView.Navigate(new Uri(youtubeHome));
        }

        private async Task CloseYoutubeBrowserAsync()
        {
            youtubeBrowseMode = false;
            RestoreYoutubePlayerHeightAfterBrowse();
            YoutubeSearchIcon.Text = "\uE721";
            ToolTipService.SetToolTip(YoutubeSearchButton, "YouTube에서 영상 찾기");
            if (!string.IsNullOrWhiteSpace(youtubeLoadedVideoId))
            {
                await LoadYoutubeVideoAsync(youtubeLoadedVideoId);
            }
            else
            {
                youtubeReady = false;
                SetYoutubeInlineState(true, "설정에서 YouTube 영상을 선택해 주세요");
            }
        }

        private async Task SelectYoutubeVideoAsync(string videoId)
        {
            if (string.IsNullOrWhiteSpace(videoId)) return;
            youtubeBrowseMode = false;
            RestoreYoutubePlayerHeightAfterBrowse();
            YoutubeSearchIcon.Text = "\uE721";
            ToolTipService.SetToolTip(YoutubeSearchButton, "YouTube에서 영상 찾기");
            youtubeLoadedVideoId = videoId;
            YoutubePreferences.Video = "https://www.youtube.com/watch?v=" + videoId;
            App.LogStage("youtube-browser-selected-" + videoId);
            await LoadYoutubeVideoAsync(videoId);
        }

        private void RestoreYoutubePlayerHeightAfterBrowse()
        {
            if (youtubePlayerHeightBeforeBrowse > 0)
                YoutubeVideoHost.Height = youtubePlayerHeightBeforeBrowse;
            youtubePlayerHeightBeforeBrowse = 0;
        }

        private void YoutubeCoreWebView2_NavigationStarting(
            Microsoft.Web.WebView2.Core.CoreWebView2 sender,
            Microsoft.Web.WebView2.Core.CoreWebView2NavigationStartingEventArgs args)
        {
            if (!youtubeBrowseMode) return;
            var videoId = ExtractYoutubeVideoId(args.Uri);
            if (string.IsNullOrWhiteSpace(videoId)) return;
            args.Cancel = true;
            _ = SelectYoutubeVideoAsync(videoId);
        }

        private void YoutubeFallbackWebView_NavigationStarting(WebView sender, WebViewNavigationStartingEventArgs args)
        {
            if (!youtubeBrowseMode || args.Uri == null) return;
            var videoId = ExtractYoutubeVideoId(args.Uri.AbsoluteUri);
            if (string.IsNullOrWhiteSpace(videoId)) return;
            args.Cancel = true;
            _ = SelectYoutubeVideoAsync(videoId);
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
                youtubeWebView2.CoreWebView2.NavigationStarting -= YoutubeCoreWebView2_NavigationStarting;
                youtubeWebView2.CoreWebView2.NavigationStarting += YoutubeCoreWebView2_NavigationStarting;
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
                SetYoutubeInlineState(true, youtubeBrowseMode ? "YouTube를 열지 못했습니다." : "YouTube 영상을 불러오지 못했습니다.\n링크를 확인해 주세요.");
                return;
            }
            if (youtubeBrowseMode)
            {
                youtubeReady = false;
                SetYoutubeInlineState(false, null);
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
                SetYoutubeInlineState(true, youtubeBrowseMode ? "YouTube를 열지 못했습니다." : "YouTube 영상을 불러오지 못했습니다.\n링크를 확인해 주세요.");
                return;
            }
            if (youtubeBrowseMode)
            {
                youtubeReady = false;
                SetYoutubeInlineState(false, null);
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
            voiceChatSignature = "";
            voiceChatFirstSeenAtMs.Clear();
            voiceChatRenderedKeys.Clear();
            voiceChatHasVisibleMessages = false;
            VoiceChatOverlay.Children.Clear();
            UpdateVoiceChatPanelChrome(widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly);
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
