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
using Microsoft.Gaming.XboxGameBar.Input;
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
        private Microsoft.UI.Xaml.Controls.WebView2 browserWebView2;
        private bool browserWebView2Unavailable;
        private bool browserUsingFallback;
        private bool browserReady;
        private string browserLoadedUrl = "";
        private bool gameBarUiaProbeInFlight;
        private bool gameBarUiaProbeCompleted;
        private int gameBarUiaProbeAttempts;
        private long lastGameBarUiaProbeAttemptAtMs;
        private bool youtubeUsingFallback;
        private bool youtubeBrowseMode;
        private double youtubePlayerHeightBeforeBrowse;
        private bool youtubeNativePointerCaptured;
        private bool youtubeNativeMouseWasDown;
        private int youtubeNativeCoordinateMode = -1;
        private double youtubeNativeLastX = double.NaN;
        private double youtubeNativeLastY = double.NaN;
        private long youtubeNativeLastMoveAtMs;
        private long youtubeNativeLastErrorLogAtMs;
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
        private FrameworkElement nativeDraggedPanel;
        private TranslateTransform nativeDraggedPanelTransform;
        private Windows.Foundation.Point nativePanelDragStartPoint;
        private double nativePanelDragStartX;
        private double nativePanelDragStartY;
        private int nativePanelDragCoordinateMode = -1;
        private bool nativePanelDragMoved;
        private int nativePanelDragActive;
        private bool nativePanelMouseWasDown;
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
        private const string BrowserPanelXSetting = "browser-panel-x";
        private const string BrowserPanelYSetting = "browser-panel-y";
        private const string BrowserPanelWidthSetting = "browser-panel-width";
        private const string BrowserPanelHeightSetting = "browser-panel-height";
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
        private readonly Dictionary<string, long> voiceChatExpiresAtMs = new Dictionary<string, long>(StringComparer.Ordinal);
        private readonly Dictionary<string, FrameworkElement> voiceChatRowsByKey = new Dictionary<string, FrameworkElement>(StringComparer.Ordinal);
        private readonly HashSet<string> voiceChatRenderedKeys = new HashSet<string>(StringComparer.Ordinal);
        private bool voiceChatHasVisibleMessages;
        private bool voiceChatPositionNeedsDefault;
        private string overlayToastSignature = "";
        private readonly Dictionary<string, long> overlayToastExpiresAtMs = new Dictionary<string, long>(StringComparer.Ordinal);
        private readonly Dictionary<string, FrameworkElement> overlayToastRowsById = new Dictionary<string, FrameworkElement>(StringComparer.Ordinal);
        private bool controlsTutorialShownThisSession;
        private long lastSnapshotSuccessAtMs;
        private int consecutiveSnapshotFailures;
        private long lastSnapshotFailureLogAtMs;
        private long lastTutorialResetGeneration;
        private bool tutorialResetAwaitingUnpin;
        private int tutorialRefreshGeneration;
        private int pinnedGrowGeneration;
        private bool pinnedGrowInFlight;
        private long resizeRequestSequence;
        private long lastResizeRequestId;
        private long lastResizeRequestAtMs;
        private string lastResizeRequestReason = "none";
        private CancellationTokenSource snapshotWebSocketLoopCts;
        private MessageWebSocket snapshotWebSocket;
        private volatile bool snapshotWebSocketConnected;
        private int snapshotWebSocketGeneration;
        private long lastSnapshotWebSocketMessageAtMs;
        private long lastSnapshotWebSocketFailureLogAtMs;
        private bool hudLoaded;
        private XboxGameBarHotkeyWatcher runtimePanelsHotkeyWatcher;
        private bool overlayUpdateRequestedForForeground;
        private long lastRuntimePanelsToggleAtMs;
        // F9 is a session-only master visibility gate. It must never rewrite
        // persisted per-panel or global HUD preferences.
        private bool runtimeOverlayHiddenByHotkey;
        private int foregroundChromeRefreshGeneration;
        private bool foregroundChromeRefreshInFlight;
        private long lastForegroundChromeRefreshAtMs;
        private bool winGPanelSettingsLoading;
        private volatile bool nativeInputDiagnosticsWanted;
        private volatile bool nativeInputRuntimeDragWanted;
        private bool nativeInputDeactivatedWhileIdle;
        private const double HiddenRuntimePanelOpacity = 0.60;
        private const double RuntimeNativeDragThreshold = 2.0;

        public Widget()
        {
            InitializeComponent();
            CursorMarkersToggle.IsOn = HudPreferences.NativeCursorMarkersVisible;
            ApplyCursorMarkersVisibility(CursorMarkersToggle.IsOn);
            ClickNativeResult.Text = "Agent native input initializing";
            SetVersionText();
            RestorePanelPositions();
            RestorePanelSizes();
            SizeChanged += (_, e) =>
            {
                var now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
                var correlated = lastResizeRequestId > 0 && now - lastResizeRequestAtMs <= 2000;
                var host = widget != null ? widget.WindowBounds : new Windows.Foundation.Rect();
                App.LogResize("size-changed previous=" + FormatSize(e.PreviousSize) +
                    " new=" + FormatSize(e.NewSize) +
                    " host=" + FormatRect(host) +
                    " requestId=" + (correlated ? lastResizeRequestId.ToString() : "manual-or-host") +
                    " reason=" + (correlated ? lastResizeRequestReason : "manual-or-host") +
                    " mode=" + (widget != null ? widget.GameBarDisplayMode.ToString() : "none") +
                    " pinned=" + (widget != null && widget.Pinned));
                EnsureDefaultVoiceChatPosition();
                ClampAllPanelPositions();
                UpdatePinTutorial();
            };
            timer.Tick += async (_, __) =>
            {
                // Voice chat expiry is a UI concern and must keep advancing even
                // while the realtime snapshot WebSocket is healthy and idle.
                ExpireVoiceChatMessages();
                ExpireOverlayToasts();
                // The WebSocket is the primary realtime transport. The legacy
                // snapshot TCP endpoint is polled only while WS is unavailable.
                if (!snapshotWebSocketConnected) await RefreshAsync();
            };
            Loaded += async (_, __) =>
            {
                App.LogStage("hud-loaded");
                hudLoaded = true;
                Window.Current.CoreWindow.KeyDown += CoreWindowKeyDown;
                Window.Current.CoreWindow.KeyUp += CoreWindowKeyUp;
                Window.Current.Activated += WindowActivated;
                ApplicationData.Current.DataChanged += PreferencesChanged;
                lastFitRequest = HudPreferences.FitRequest;
                ApplyPreferences();
                EnsureDefaultVoiceChatPosition();
                // The manifest size is only a safe bootstrap. Resolve the real default
                // from the current monitor and the host's accepted/on-screen bounds.
                // This avoids a 1440x900 machine-specific initial size.
                if (!HudPreferences.InitialMaxFitDone)
                {
                    var initialFitAccepted = await FitToLargestOnscreenAsync("initial-max");
                    if (initialFitAccepted) HudPreferences.MarkInitialMaxFitDone();
                }
                UpdateInputState();
                UpdatePinTutorial();
                QueueTutorialStateRefresh("loaded");
                StartSnapshotWebSocketLoop();
                timer.Start();
                // One immediate legacy fetch keeps compatibility with older Agents.
                // Once WS connects the timer stops issuing snapshot requests.
                await RefreshAsync();
                QueueTutorialStateRefresh("ready");
                QueueForegroundChromeRefresh("loaded");
                App.LogStage("hud-ready");
            };
            Unloaded += (_, __) =>
            {
                App.LogStage("hud-unloaded");
                hudLoaded = false;
                Window.Current.CoreWindow.KeyDown -= CoreWindowKeyDown;
                Window.Current.CoreWindow.KeyUp -= CoreWindowKeyUp;
                nativeInputDiagnosticsWanted = false;
                Window.Current.Activated -= WindowActivated;
                ApplicationData.Current.DataChanged -= PreferencesChanged;
                // Keep the diagnostic loop alive across transient Game Bar unload/reload cycles.
                // The watchdog will recover it if the host pauses or drops dispatcher work.
                _ = SyncExternalClickTestAsync(false);
                Interlocked.Increment(ref tutorialRefreshGeneration);
                Interlocked.Increment(ref pinnedGrowGeneration);
                Interlocked.Increment(ref foregroundChromeRefreshGeneration);
                StopSnapshotWebSocketLoop();
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
            if (args.VirtualKey == VirtualKey.Shift)
            {
                YoutubeDragSurface.IsHitTestVisible = true;
                return;
            }
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


        private void CoreWindowKeyUp(CoreWindow sender, KeyEventArgs args)
        {
            if (args.VirtualKey != VirtualKey.Shift) return;
            if (draggedPanel != YoutubePanel)
                YoutubeDragSurface.IsHitTestVisible = false;
        }


        private void ApplyPreferences()
        {
            var hudVisible = HudPreferences.Visible;
            var pinnedOnly = widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly;
            // F9 is a transient master gate for the real pinned overlay. It is
            // deliberately independent from HudPreferences.Visible and every
            // per-panel Visible/Closed setting so Win+G remains a stable editor.
            var effectiveHudVisible = hudVisible && (!pinnedOnly || !runtimeOverlayHiddenByHotkey);
            // Win+G is always an edit surface: F9 changes only PinnedOnly output and
            // therefore cannot visually mutate the editor or its saved toggles.
            HudPanel.Visibility = pinnedOnly && !effectiveHudVisible ? Visibility.Collapsed : Visibility.Visible;
            HudPanel.Opacity = HudPreferences.Opacity;
            RootGrid.Background = effectiveHudVisible && HudPreferences.CoverageTint
                ? new SolidColorBrush(Color.FromArgb(26, 123, 90, 166))
                : new SolidColorBrush(Colors.Transparent);
            ApplyCursorMarkersVisibility(effectiveHudVisible && HudPreferences.NativeCursorMarkersVisible);
            ApplyVoiceChatPreferences();
            UpdatePanelChrome();
            RefreshWinGPanelSettings();
            ApplyYoutubePreferences();
            ApplyBrowserPreferences();
            // When the built-in YouTube browser is open, an empty saved URL is
            // expected: the user may be searching/browsing for a video there.
            // Do not cover that browser with the legacy "enter a link" prompt.
            if (!youtubeBrowseMode &&
                string.IsNullOrWhiteSpace(ExtractYoutubeVideoId(YoutubePreferences.Video)))
                SetYoutubeInlineState(true, "영상 링크를 입력해 주세요");
        }

        private void ApplyYoutubePreferences()
        {
            var configuredVideo = YoutubePreferences.Video;
            var configuredId = ExtractYoutubeVideoId(configuredVideo);
            var configuredVolume = YoutubePreferences.Volume;
            youtubeVolume = configuredVolume;
            if (youtubeReady) _ = SetYoutubeVolumeAsync(configuredVolume);

            if (!string.IsNullOrWhiteSpace(configuredId) &&
                !string.Equals(configuredId, youtubeLoadedVideoId, StringComparison.Ordinal) &&
                ShouldActivateYoutubeRuntime())
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
                            _ = FitToDisplayAsync();
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
                StartRuntimePanelsHotkey();
                RequestImmediateOverlayUpdateIfForeground("navigated");
                UpdateNativeInputDiagnosticsMode(true);
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
                StopRuntimePanelsHotkey();
                widget.SettingsClicked -= WidgetSettingsClicked;
                widget.WindowBoundsChanged -= WidgetStateChanged;
                widget.PinnedChanged -= WidgetStateChanged;
                widget.ClickThroughEnabledChanged -= WidgetStateChanged;
                widget.GameBarDisplayModeChanged -= DisplayModeChanged;
            }
            notificationManager = null;
            base.OnNavigatedFrom(e);
        }

        private void StartRuntimePanelsHotkey()
        {
            if (widget == null || runtimePanelsHotkeyWatcher != null) return;
            try
            {
                runtimePanelsHotkeyWatcher = XboxGameBarHotkeyWatcher.CreateWatcher(
                    widget, new List<VirtualKey> { VirtualKey.F9 });
                runtimePanelsHotkeyWatcher.HotkeySetStateChanged += RuntimePanelsHotkeyChanged;
                runtimePanelsHotkeyWatcher.Start();
                App.LogResize("runtime-panels-hotkey-started key=F9");
            }
            catch (Exception ex)
            {
                App.LogResize("runtime-panels-hotkey-error stage=start error=" + ErrorCode(ex));
            }
        }

        private void StopRuntimePanelsHotkey()
        {
            try
            {
                if (runtimePanelsHotkeyWatcher != null)
                {
                    runtimePanelsHotkeyWatcher.HotkeySetStateChanged -= RuntimePanelsHotkeyChanged;
                    runtimePanelsHotkeyWatcher.Stop();
                }
            }
            catch { }
            runtimePanelsHotkeyWatcher = null;
        }

        private async void RuntimePanelsHotkeyChanged(XboxGameBarHotkeyWatcher sender, HotkeySetStateChangedArgs args)
        {
            if (!args.HotkeySetDown) return;
            try
            {
                await Dispatcher.RunAsync(CoreDispatcherPriority.Normal, () => ToggleRuntimePanelsVisibility("gamebar-hotkey"));
            }
            catch (Exception ex)
            {
                App.LogResize("runtime-panels-hotkey-error stage=dispatch error=" + ErrorCode(ex));
            }
        }

        private void ToggleRuntimePanelsVisibility(string source = "gamebar")
        {
            // F9 can arrive through both XboxGameBarHotkeyWatcher and the Agent native
            // input sequence. Treat both as one physical key press.
            var now = NowMs();
            if (now - lastRuntimePanelsToggleAtMs < 350)
            {
                App.LogResize("runtime-panels-toggle-duplicate source=" + source +
                    " ageMs=" + (now - lastRuntimePanelsToggleAtMs));
                return;
            }
            lastRuntimePanelsToggleAtMs = now;

            // F9 toggles only a transient runtime master gate. Persisted settings
            // (global HUD enabled, panel visibility, panel closed state) are untouched.
            // This means pressing F9 while Win+G is open cannot rewrite the editor.
            runtimeOverlayHiddenByHotkey = !runtimeOverlayHiddenByHotkey;
            ApplyPreferences();
            App.LogResize("runtime-overlay-hotkey-toggle source=" + source +
                " hidden=" + runtimeOverlayHiddenByHotkey +
                " configured=" + HudPreferences.Visible +
                " mode=" + (widget != null ? widget.GameBarDisplayMode.ToString() : "none") +
                " pinned=" + (widget != null && widget.Pinned));
        }

        private void RequestImmediateOverlayUpdateIfForeground(string reason)
        {
            if (widget == null) return;
            if (widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly)
            {
                overlayUpdateRequestedForForeground = false;
                return;
            }
            if (overlayUpdateRequestedForForeground) return;
            overlayUpdateRequestedForForeground = true;
            _ = RequestImmediateOverlayUpdateAsync(reason);
        }

        private async Task RequestImmediateOverlayUpdateAsync(string reason)
        {
            try
            {
                var payload = new JsonObject
                {
                    ["command"] = JsonValue.CreateStringValue("check_overlay_update"),
                    ["forceFinalize"] = JsonValue.CreateBooleanValue(true),
                    ["reason"] = JsonValue.CreateStringValue(reason ?? "foreground")
                }.Stringify();
                var response = await SendAgentCommandAsync(payload);
                App.LogResize("overlay-update-foreground-request reason=" + reason +
                    " response=" + (response ?? ""));
            }
            catch (Exception ex)
            {
                // Allow a later display-mode callback in the same session to retry
                // if the Agent was still starting when Win+G opened.
                overlayUpdateRequestedForForeground = false;
                App.LogResize("overlay-update-foreground-error reason=" + reason +
                    " error=" + ErrorCode(ex));
            }
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
            });
        }

        private async void DisplayModeChanged(XboxGameBarWidget sender, object args)
        {
            await Dispatcher.RunAsync(CoreDispatcherPriority.Normal, () =>
            {
                if (widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly)
                {
                    // A completed auto-setup is valid only for the current Win+G foreground
                    // session. Closing Game Bar rearms selection/pin/click-through setup for
                    // the next Win+G open.
                    gameBarUiaProbeCompleted = false;
                    gameBarUiaProbeAttempts = 0;
                    lastGameBarUiaProbeAttemptAtMs = 0;
                }
                RequestImmediateOverlayUpdateIfForeground("display-mode-changed");
                QueueForegroundChromeRefresh("display-mode-changed");
                // 856 DIP is only a conservative bootstrap height for affected Game Bar
                // hosts. The real pinned target comes from the current display's logical
                // dimensions, so 1080p/1440p/etc. are not capped to this machine's 900 DIP.
                if (!HudPreferences.CompactLayout && widget.Pinned &&
                    widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly)
                    QueuePinnedGrowToFullscreen();
                UpdateDisplayState();
                UpdateInputState();
                RefreshWinGPanelSettings();
                UpdatePinTutorial();
            });
        }

        private void QueueForegroundChromeRefresh(string reason)
        {
            // The host-owned title/pin chrome can stay stale after a package update
            // or a fresh Win+G foreground activation. This also affects widgets that
            // are already pinned: while Win+G is open they are still in Foreground and
            // need editable geometry that leaves room for the host title/pin bar.
            if (widget == null || !hudLoaded ||
                widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly) return;

            var now = NowMs();
            if (now - lastForegroundChromeRefreshAtMs < 750) return;
            lastForegroundChromeRefreshAtMs = now;
            var generation = Interlocked.Increment(ref foregroundChromeRefreshGeneration);
            _ = RefreshForegroundChromeAfterHostSettlesAsync(generation, reason);
        }

        private async Task RefreshForegroundChromeAfterHostSettlesAsync(int generation, string reason)
        {
            await Task.Delay(90);
            if (generation != Volatile.Read(ref foregroundChromeRefreshGeneration)) return;
            if (widget == null || !hudLoaded ||
                widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly) return;
            if (foregroundChromeRefreshInFlight || fittingDisplay) return;

            foregroundChromeRefreshInFlight = true;
            try
            {
                widget.PinningSupported = true;
                var bounds = widget.WindowBounds;
                if (bounds.Width <= 0 || bounds.Height <= 0) return;

                // Do not move a valid user-positioned window. Only if the host restored
                // it partly off-screen, recompute the largest size that the current
                // monitor can keep at non-negative coordinates.
                if (bounds.X < -0.25 || bounds.Y < -0.25)
                    await FitToLargestOnscreenAsync("foreground-" + reason);

                App.LogResize("foreground-chrome-refresh reason=" + reason +
                    " actual=" + (widget != null ? FormatRect(widget.WindowBounds) : "none"));
            }
            catch (Exception ex)
            {
                App.LogResize("foreground-chrome-refresh-error reason=" + reason +
                    " error=" + ErrorCode(ex));
            }
            finally
            {
                foregroundChromeRefreshInFlight = false;
            }
        }

        private async Task<bool> FitToLargestOnscreenAsync(string reason)
        {
            if (widget == null || fittingDisplay) return false;
            fittingDisplay = true;
            try
            {
                UpdateDisplayMetrics();
                var minimum = widget.MinWindowSize;
                var maximum = widget.MaxWindowSize;
                var targetWidth = Math.Max(minimum.Width, Math.Min(maximum.Width, displayWidth));
                var targetHeight = Math.Max(minimum.Height, Math.Min(maximum.Height, displayHeight));

                var result = await HudGeometry.FitLargestAsync(
                    targetWidth, targetHeight, minimum.Width, minimum.Height,
                    async (width, height) =>
                    {
                        if (widget == null || widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly)
                            return false;
                        var accepted = await ResizeHudAsync(width, height, "onscreen-max-" + reason);
                        if (!accepted || widget == null) return false;
                        await Task.Delay(25);
                        var actual = widget.WindowBounds;
                        var onScreen = actual.X >= -0.25 && actual.Y >= -0.25;
                        App.LogResize("onscreen-max-probe reason=" + reason +
                            " requested=" + FormatSize(width, height) +
                            " actual=" + FormatRect(actual) +
                            " onScreen=" + onScreen);
                        return onScreen;
                    });

                resizeAccepted = result.Accepted;
                fittedWidth = result.Width;
                fittedHeight = result.Height;
                fitTargetWidth = targetWidth;
                fitTargetHeight = targetHeight;
                UpdateDisplayState();
                App.LogResize("onscreen-max-result reason=" + reason +
                    " accepted=" + result.Accepted +
                    " fitted=" + FormatSize(result.Width, result.Height) +
                    " target=" + FormatSize(targetWidth, targetHeight) +
                    " actual=" + (widget != null ? FormatRect(widget.WindowBounds) : "none"));
                return result.Accepted;
            }
            catch (Exception ex)
            {
                App.LogResize("onscreen-max-error reason=" + reason + " error=" + ErrorCode(ex));
                return false;
            }
            finally
            {
                fittingDisplay = false;
            }
        }

        private void QueuePinnedGrowToFullscreen()
        {
            var generation = Interlocked.Increment(ref pinnedGrowGeneration);
            var current = widget != null ? widget.WindowBounds : new Windows.Foundation.Rect();
            App.LogResize("pinned-grow-queued generation=" + generation +
                " current=" + FormatRect(current) +
                " mode=" + (widget != null ? widget.GameBarDisplayMode.ToString() : "none") +
                " pinned=" + (widget != null && widget.Pinned));
            _ = GrowPinnedToFullscreenAsync(generation);
        }

        private async Task GrowPinnedToFullscreenAsync(int generation)
        {
            // The first PinnedOnly callback can still inherit the ~820 DIP chrome
            // restriction. Retry after the host settles instead of accepting 820 as
            // a permanent maximum. 856 is known to be reachable on this layout.
            var delays = new[] { 80, 180, 350, 700, 1200, 2000 };
            foreach (var delayMs in delays)
            {
                await Task.Delay(delayMs);
                if (generation != Volatile.Read(ref pinnedGrowGeneration)) return;
                if (widget == null || !hudLoaded || HudPreferences.CompactLayout ||
                    !widget.Pinned || widget.GameBarDisplayMode != XboxGameBarDisplayMode.PinnedOnly) return;
                if (pinnedGrowInFlight) continue;

                pinnedGrowInFlight = true;
                try
                {
                    UpdateDisplayMetrics();
                    var maximum = widget.MaxWindowSize;
                    var current = widget.WindowBounds;
                    var targetWidth = Math.Min(maximum.Width, displayWidth);
                    var targetHeight = Math.Min(maximum.Height, displayHeight);
                    var knownGoodHeight = Math.Min(targetHeight, 856.0);

                    // Preserve any manual size already above the known-good height.
                    // Otherwise first establish 856, then probe toward the actual display height.
                    var baseWidth = Math.Min(targetWidth, Math.Max(current.Width, 1.0));
                    var baseHeight = Math.Min(targetHeight, Math.Max(current.Height, 1.0));
                    if (baseHeight < knownGoodHeight)
                    {
                        var safeAccepted = await ResizeHudAsync(baseWidth, knownGoodHeight, "pinned-grow-safe");
                        App.LogStage("pinned-grow-safe delayMs=" + delayMs +
                            " accepted=" + safeAccepted + " from=" + current.Width.ToString("0.#") + "x" + current.Height.ToString("0.#") +
                            " safe=" + baseWidth.ToString("0.#") + "x" + knownGoodHeight.ToString("0.#"));
                        if (!safeAccepted) continue;
                        baseHeight = knownGoodHeight;
                        current = widget.WindowBounds;
                    }

                    App.LogResize("pinned-grow-attempt delayMs=" + delayMs +
                        " current=" + FormatRect(current) +
                        " target=" + FormatSize(targetWidth, targetHeight) +
                        " knownGoodHeight=" + knownGoodHeight.ToString("0.#") +
                        " display=" + FormatSize(displayWidth, displayHeight) +
                        " min=" + FormatSize(widget.MinWindowSize) +
                        " max=" + FormatSize(maximum));
                    var result = await HudGeometry.FitLargestUpwardAsync(
                        targetWidth, targetHeight, current.Width, Math.Max(baseHeight, current.Height),
                        (w, h) => ResizeHudAsync(w, h, "pinned-grow-probe"));
                    resizeAccepted = result.Accepted;
                    fittedWidth = result.Width;
                    fittedHeight = result.Height;
                    fitTargetWidth = targetWidth;
                    fitTargetHeight = targetHeight;
                    UpdateDisplayState();
                    App.LogStage("pinned-grow-probe delayMs=" + delayMs +
                        " result=" + result.Width.ToString("0.#") + "x" + result.Height.ToString("0.#") +
                        " target=" + targetWidth.ToString("0.#") + "x" + targetHeight.ToString("0.#"));

                    if (result.Width >= targetWidth - 1.0 && result.Height >= targetHeight - 1.0) return;
                }
                catch (Exception ex)
                {
                    App.LogStage("pinned-grow-error delayMs=" + delayMs + " error=" + ErrorCode(ex));
                }
                finally
                {
                    pinnedGrowInFlight = false;
                }
            }
        }

        private async Task FitToDisplayAsync()
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
                if (HudPreferences.CompactLayout)
                {
                    resizeAccepted = await ResizeHudAsync(width, height, "fit-compact");
                    if (resizeAccepted)
                    {
                        fittedWidth = width;
                        fittedHeight = height;
                    }
                }
                else
                {
                    // Search upward only. The current host size is the hard lower
                    // bound, so a transient chrome restriction can never push the HUD
                    // back down (for example from a manual 1435x860 to 1435x820).
                    var current = widget.WindowBounds;
                    App.LogResize("fit-upward-start current=" + FormatRect(current) +
                        " target=" + FormatSize(width, height) +
                        " display=" + FormatSize(displayWidth, displayHeight) +
                        " min=" + FormatSize(minimum) +
                        " max=" + FormatSize(maximum));
                    var result = await HudGeometry.FitLargestUpwardAsync(
                        width, height, current.Width, current.Height,
                        (w, h) => ResizeHudAsync(w, h, "fit-upward-probe"));
                    resizeAccepted = result.Accepted;
                    fittedWidth = result.Width;
                    fittedHeight = result.Height;
                }
                // Never move the window automatically; preserve the user/host position.
                UpdateDisplayState();
            }
            catch (Exception ex)
            {
                DisplayText.Text = "화면 맞춤 실패 · " + ErrorCode(ex);
            }
            finally { fittingDisplay = false; }
        }

        private Task<bool> ResizeHudAsync(double width, double height)
        {
            return ResizeHudAsync(width, height, "fit");
        }

        private async Task<bool> ResizeHudAsync(double width, double height, string reason)
        {
            if (widget == null) return false;
            var requestId = Interlocked.Increment(ref resizeRequestSequence);
            var before = widget.WindowBounds;
            var minimum = widget.MinWindowSize;
            var maximum = widget.MaxWindowSize;
            var requestReason = reason ?? "unknown";
            lastResizeRequestId = requestId;
            lastResizeRequestAtMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
            lastResizeRequestReason = requestReason;
            App.LogResize("resize-request id=" + requestId +
                " reason=" + requestReason +
                " current=" + FormatRect(before) +
                " target=" + FormatSize(width, height) +
                " min=" + FormatSize(minimum) +
                " max=" + FormatSize(maximum) +
                " display=" + FormatSize(displayWidth, displayHeight) +
                " scale=" + displayScale.ToString("0.###") +
                " mode=" + widget.GameBarDisplayMode +
                " pinned=" + widget.Pinned +
                " compact=" + HudPreferences.CompactLayout);
            bool accepted;
            try
            {
                accepted = await widget.TryResizeWindowAsync(new Windows.Foundation.Size(width, height));
            }
            catch (Exception ex)
            {
                App.LogResize("resize-error id=" + requestId +
                    " reason=" + requestReason +
                    " target=" + FormatSize(width, height) +
                    " error=" + ErrorCode(ex));
                throw;
            }
            var after = widget.WindowBounds;
            App.LogResize("resize-result id=" + requestId +
                " reason=" + requestReason +
                " accepted=" + accepted +
                " requested=" + FormatSize(width, height) +
                " actual=" + FormatRect(after) +
                " delta=" + FormatSize(after.Width - before.Width, after.Height - before.Height) +
                " mode=" + widget.GameBarDisplayMode +
                " pinned=" + widget.Pinned);
            App.LogStage($"fit {widget.GameBarDisplayMode} {width}x{height} accepted={accepted}");
            return accepted;
        }

        private static string FormatSize(Windows.Foundation.Size size)
        {
            return FormatSize(size.Width, size.Height);
        }

        private static string FormatSize(double width, double height)
        {
            return width.ToString("0.##") + "x" + height.ToString("0.##");
        }

        private static string FormatRect(Windows.Foundation.Rect rect)
        {
            return rect.Width.ToString("0.##") + "x" + rect.Height.ToString("0.##") +
                "@" + rect.X.ToString("0.##") + "," + rect.Y.ToString("0.##");
        }

        private void UpdateDisplayMetrics()
        {
            var display = DisplayInformation.GetForCurrentView();
            displayScale = display.RawPixelsPerViewPixel;
            displayWidth = HudGeometry.ToLogicalPixels(display.ScreenWidthInRawPixels, displayScale);
            displayHeight = HudGeometry.ToLogicalPixels(display.ScreenHeightInRawPixels, displayScale);
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
                    QueueForegroundChromeRefresh("tutorial-host-settled-" + reason);
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
            RequestGameBarUiaProbeIfNeeded(pinnedOnly);
            SaveStatus();
        }

        private void RequestGameBarUiaProbeIfNeeded(bool pinnedOnly)
        {
            if (widget == null || gameBarUiaProbeCompleted || gameBarUiaProbeInFlight) return;
            // Probe whenever Win+G is foreground. Do not gate on the current
            // click-through value: the diagnostic also needs to identify the
            // toolbar button when click-through is already enabled.
            if (pinnedOnly) return;
            var now = NowMs();
            if (now - lastGameBarUiaProbeAttemptAtMs < 750) return;
            lastGameBarUiaProbeAttemptAtMs = now;
            gameBarUiaProbeInFlight = true;
            gameBarUiaProbeAttempts++;
            _ = ProbeGameBarUiaAsync(gameBarUiaProbeAttempts);
        }

        private async Task ProbeGameBarUiaAsync(int attempt)
        {
            bool success = false;
            try
            {
                // Let the Game Bar toolbar finish entering the automation tree.
                await Task.Delay(attempt == 1 ? 250 : 500);
                if (widget == null || widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly) return;
                var response = await SendAgentCommandAsync("{\"command\":\"probe_gamebar_uia\"}");
                bool commandOk = false;
                bool gameBarFound = false;
                bool yummiSelected = false;
                bool selectionInvoked = false;
                bool pinEnabled = false;
                bool clickThroughEnabled = false;
                try
                {
                    var parsed = JsonObject.Parse(response ?? "{}");
                    commandOk = parsed.GetNamedBoolean("ok", false);
                    var data = parsed.GetNamedObject("data", new JsonObject());
                    gameBarFound = data.GetNamedBoolean("gameBarFound", false);
                    yummiSelected = data.GetNamedBoolean("yummiSelected", false);
                    selectionInvoked = data.GetNamedBoolean("selectionInvoked", false);
                    pinEnabled = data.GetNamedObject("pin", new JsonObject()).GetNamedBoolean("enabled", false);
                    clickThroughEnabled = data.GetNamedObject("clickThrough", new JsonObject()).GetNamedBoolean("enabled", false);
                    success = commandOk && data.GetNamedBoolean("ready", false);
                }
                catch { success = false; }
                App.LogStage(
                    "gamebar-uia-auto-attempt-" + attempt +
                    " command=" + commandOk +
                    " gamebar=" + gameBarFound +
                    " selected=" + yummiSelected +
                    " selectionInvoked=" + selectionInvoked +
                    " pin=" + pinEnabled +
                    " clickThrough=" + clickThroughEnabled +
                    " ready=" + success);
                if (success) gameBarUiaProbeCompleted = true;
            }
            catch (Exception ex)
            {
                App.LogStage("gamebar-uia-probe-failed-attempt-" + attempt + "-" + ErrorCode(ex));
            }
            finally
            {
                gameBarUiaProbeInFlight = false;
            }

            // Agent startup/update can race Win+G. Retry a few times in the same
            // foreground session instead of permanently suppressing the probe.
            if (!success && !gameBarUiaProbeCompleted && attempt < 6)
            {
                await Task.Delay(900);
                try
                {
                    await Dispatcher.RunAsync(CoreDispatcherPriority.Low, () =>
                    {
                        if (widget != null)
                            RequestGameBarUiaProbeIfNeeded(widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly);
                    });
                }
                catch { }
            }
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
                    ["displayHeight"] = JsonValue.CreateNumberValue(displayHeight),
                    ["snapshotTransport"] = JsonValue.CreateStringValue(snapshotWebSocketConnected ? "ws" : "tcp-fallback"),
                    ["snapshotWsLastMessageAgeMs"] = JsonValue.CreateNumberValue(
                        lastSnapshotWebSocketMessageAtMs > 0 ? Math.Max(0, NowMs() - lastSnapshotWebSocketMessageAtMs) : -1)
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

        private void StartSnapshotWebSocketLoop()
        {
            StopSnapshotWebSocketLoop();
            var cts = new CancellationTokenSource();
            snapshotWebSocketLoopCts = cts;
            var generation = Interlocked.Increment(ref snapshotWebSocketGeneration);
            _ = SnapshotWebSocketLoopAsync(generation, cts.Token);
        }

        private void StopSnapshotWebSocketLoop()
        {
            Interlocked.Increment(ref snapshotWebSocketGeneration);
            var cts = snapshotWebSocketLoopCts;
            snapshotWebSocketLoopCts = null;
            if (cts != null)
            {
                try { cts.Cancel(); } catch { }
                cts.Dispose();
            }
            var socket = snapshotWebSocket;
            snapshotWebSocket = null;
            snapshotWebSocketConnected = false;
            if (socket != null)
            {
                try { socket.Dispose(); } catch { }
            }
        }

        private async Task SnapshotWebSocketLoopAsync(int generation, CancellationToken token)
        {
            while (!token.IsCancellationRequested && generation == Volatile.Read(ref snapshotWebSocketGeneration))
            {
                var socket = new MessageWebSocket();
                var closed = new TaskCompletionSource<bool>();
                socket.Control.MessageType = SocketMessageType.Utf8;
                socket.MessageReceived += SnapshotWebSocket_MessageReceived;
                socket.Closed += (_, args) =>
                {
                    if (generation == Volatile.Read(ref snapshotWebSocketGeneration))
                    {
                        snapshotWebSocketConnected = false;
                        App.LogStage("snapshot-ws-closed code=" + args.Code);
                    }
                    closed.TrySetResult(true);
                };

                try
                {
                    await socket.ConnectAsync(new Uri("ws://127.0.0.1:19517/")).AsTask(token);
                    if (token.IsCancellationRequested || generation != Volatile.Read(ref snapshotWebSocketGeneration))
                        break;
                    snapshotWebSocket = socket;
                    snapshotWebSocketConnected = true;
                    App.LogStage("snapshot-ws-connected");

                    using (token.Register(() => closed.TrySetCanceled()))
                    {
                        try { await closed.Task; }
                        catch (TaskCanceledException) { }
                    }
                }
                catch (OperationCanceledException) when (token.IsCancellationRequested)
                {
                    break;
                }
                catch (Exception ex)
                {
                    if (generation == Volatile.Read(ref snapshotWebSocketGeneration))
                    {
                        snapshotWebSocketConnected = false;
                        var now = NowMs();
                        if (now - lastSnapshotWebSocketFailureLogAtMs >= 5000)
                        {
                            lastSnapshotWebSocketFailureLogAtMs = now;
                            App.LogStage("snapshot-ws-connect-failed error=" + ErrorCode(ex));
                        }
                    }
                }
                finally
                {
                    socket.MessageReceived -= SnapshotWebSocket_MessageReceived;
                    if (ReferenceEquals(snapshotWebSocket, socket)) snapshotWebSocket = null;
                    if (generation == Volatile.Read(ref snapshotWebSocketGeneration))
                        snapshotWebSocketConnected = false;
                    try { socket.Dispose(); } catch { }
                }

                if (token.IsCancellationRequested || generation != Volatile.Read(ref snapshotWebSocketGeneration))
                    break;
                try { await Task.Delay(500, token); }
                catch (TaskCanceledException) { break; }
            }
        }

        private async void SnapshotWebSocket_MessageReceived(
            MessageWebSocket sender, MessageWebSocketMessageReceivedEventArgs args)
        {
            string payload;
            try
            {
                using (var reader = args.GetDataReader())
                {
                    reader.UnicodeEncoding = Windows.Storage.Streams.UnicodeEncoding.Utf8;
                    payload = reader.ReadString(reader.UnconsumedBufferLength);
                }
            }
            catch (Exception ex)
            {
                App.LogStage("snapshot-ws-read-failed error=" + ErrorCode(ex));
                return;
            }
            if (string.IsNullOrWhiteSpace(payload)) return;

            await Dispatcher.RunAsync(CoreDispatcherPriority.High, () =>
            {
                try
                {
                    Apply(JsonObject.Parse(payload));
                    lastSnapshotWebSocketMessageAtMs = NowMs();
                    lastSnapshotSuccessAtMs = lastSnapshotWebSocketMessageAtMs;
                    consecutiveSnapshotFailures = 0;
                }
                catch (Exception ex)
                {
                    App.LogStage("snapshot-ws-apply-failed error=" + ErrorCode(ex));
                }
            });
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
            // Keep the last good snapshot visible while the fast local snapshot poll retries.
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
            ApplyOverlayToasts(json);
            SaveStatus();
        }

        private void ApplyOverlayToasts(JsonObject json)
        {
            JsonArray values = null;
            if (json.ContainsKey("overlay_toasts") && json["overlay_toasts"].ValueType == JsonValueType.Array)
                values = json.GetNamedArray("overlay_toasts");

            var now = NowMs();
            var visible = new List<Tuple<string, JsonObject, long, long>>();
            if (values != null)
            {
                foreach (var value in values)
                {
                    if (value.ValueType != JsonValueType.Object) continue;
                    var item = value.GetObject();
                    var title = SnapshotJson.String(item, "title");
                    var body = SnapshotJson.String(item, "body");
                    if (string.IsNullOrWhiteSpace(title) && string.IsNullOrWhiteSpace(body)) continue;
                    var timestampMs = (long)Math.Max(0, item.GetNamedNumber("timestamp_ms", 0));
                    var expiresAtMs = (long)Math.Max(0, item.GetNamedNumber("expires_at_ms", 0));
                    if (expiresAtMs <= 0 && timestampMs > 0) expiresAtMs = timestampMs + 6000;
                    if (expiresAtMs > 0 && now >= expiresAtMs) continue;
                    var id = SnapshotJson.String(item, "id");
                    if (string.IsNullOrWhiteSpace(id))
                        id = SnapshotJson.String(item, "source") + ":" + timestampMs + ":" + title + ":" + body;
                    visible.Add(Tuple.Create(id, item, expiresAtMs, timestampMs));
                }
            }

            visible = visible
                .OrderByDescending(item => item.Item4)
                .Take(4)
                .ToList();
            var signature = string.Join("|", visible.Select(item => item.Item1 + ":" + item.Item3));
            if (signature == overlayToastSignature) return;
            overlayToastSignature = signature;
            overlayToastExpiresAtMs.Clear();
            overlayToastRowsById.Clear();
            OverlayToastHost.Children.Clear();

            foreach (var item in visible)
            {
                var row = BuildOverlayToastRow(item.Item1, item.Item2);
                OverlayToastHost.Children.Add(row);
                overlayToastRowsById[item.Item1] = row;
                overlayToastExpiresAtMs[item.Item1] = item.Item3;
            }
            OverlayToastHost.Visibility = OverlayToastHost.Children.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        }

        private FrameworkElement BuildOverlayToastRow(string id, JsonObject item)
        {
            var tone = SnapshotJson.String(item, "tone", "info");
            var title = SnapshotJson.String(item, "title");
            var body = SnapshotJson.String(item, "body");
            var accent = OverlayToastAccent(tone);
            var panel = new Border
            {
                Tag = id,
                Padding = new Thickness(12, 10, 12, 10),
                CornerRadius = new CornerRadius(9),
                Background = new SolidColorBrush(Color.FromArgb(238, 20, 16, 32)),
                BorderBrush = new SolidColorBrush(accent),
                BorderThickness = new Thickness(1)
            };
            var grid = new Grid { ColumnSpacing = 10 };
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(8) });
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            var dot = new Border
            {
                Width = 7,
                Height = 7,
                CornerRadius = new CornerRadius(4),
                Background = new SolidColorBrush(accent),
                VerticalAlignment = VerticalAlignment.Top,
                Margin = new Thickness(0, 5, 0, 0)
            };
            Grid.SetColumn(dot, 0);
            grid.Children.Add(dot);
            var text = new StackPanel { Spacing = 2 };
            if (!string.IsNullOrWhiteSpace(title))
            {
                text.Children.Add(new TextBlock
                {
                    Text = title,
                    FontSize = 11,
                    FontWeight = Windows.UI.Text.FontWeights.SemiBold,
                    Foreground = new SolidColorBrush(Colors.White),
                    TextTrimming = TextTrimming.CharacterEllipsis
                });
            }
            if (!string.IsNullOrWhiteSpace(body))
            {
                text.Children.Add(new TextBlock
                {
                    Text = body,
                    FontSize = 11,
                    Foreground = new SolidColorBrush(Color.FromArgb(255, 214, 205, 226)),
                    TextWrapping = TextWrapping.Wrap,
                    MaxLines = 2
                });
            }
            Grid.SetColumn(text, 1);
            grid.Children.Add(text);
            panel.Child = grid;
            return panel;
        }

        private static Color OverlayToastAccent(string tone)
        {
            switch ((tone ?? "").Trim().ToLowerInvariant())
            {
                case "success": return Color.FromArgb(255, 86, 221, 139);
                case "warning": return Color.FromArgb(255, 245, 190, 76);
                case "error": return Color.FromArgb(255, 239, 91, 91);
                default: return Color.FromArgb(255, 151, 112, 207);
            }
        }

        private void ExpireOverlayToasts()
        {
            if (overlayToastExpiresAtMs.Count == 0) return;
            var now = NowMs();
            var expired = overlayToastExpiresAtMs
                .Where(item => item.Value > 0 && now >= item.Value)
                .Select(item => item.Key)
                .ToList();
            if (expired.Count == 0) return;
            foreach (var id in expired)
            {
                FrameworkElement row;
                if (overlayToastRowsById.TryGetValue(id, out row))
                    OverlayToastHost.Children.Remove(row);
                overlayToastRowsById.Remove(id);
                overlayToastExpiresAtMs.Remove(id);
            }
            overlayToastSignature = string.Join("|", overlayToastRowsById.Keys.OrderBy(value => value));
            OverlayToastHost.Visibility = OverlayToastHost.Children.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        }

        private void ClearOverlayToasts()
        {
            overlayToastSignature = "";
            overlayToastExpiresAtMs.Clear();
            overlayToastRowsById.Clear();
            OverlayToastHost.Children.Clear();
            OverlayToastHost.Visibility = Visibility.Collapsed;
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
                voiceChatExpiresAtMs.Clear();
                voiceChatRowsByKey.Clear();
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
                var messageTimestampMs = (long)Math.Max(0, item.GetNamedNumber("timestamp_ms", 0));
                var lifetimeOriginMs = messageTimestampMs > 0 ? messageTimestampMs : firstSeen;
                var expiresAtMs = lifetimeOriginMs > long.MaxValue - lifetimeMs ? long.MaxValue : lifetimeOriginMs + lifetimeMs;
                voiceChatExpiresAtMs[key] = expiresAtMs;
                if (now < expiresAtMs) visible.Add(Tuple.Create(key, item));
            }

            foreach (var key in voiceChatFirstSeenAtMs.Keys.Where(key => !snapshotKeys.Contains(key)).ToList())
            {
                voiceChatFirstSeenAtMs.Remove(key);
                voiceChatExpiresAtMs.Remove(key);
                voiceChatRowsByKey.Remove(key);
            }

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
            voiceChatRowsByKey.Clear();
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
                voiceChatRowsByKey[key] = row;
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


        private void ExpireVoiceChatMessages()
        {
            if (voiceChatExpiresAtMs.Count == 0) return;
            var now = NowMs();
            var expired = voiceChatExpiresAtMs
                .Where(pair => now >= pair.Value)
                .Select(pair => pair.Key)
                .ToList();
            if (expired.Count == 0) return;

            foreach (var key in expired)
            {
                voiceChatExpiresAtMs.Remove(key);
                voiceChatFirstSeenAtMs.Remove(key);
                voiceChatRenderedKeys.Remove(key);
                FrameworkElement row;
                if (voiceChatRowsByKey.TryGetValue(key, out row))
                {
                    VoiceChatOverlay.Children.Remove(row);
                    voiceChatRowsByKey.Remove(key);
                }
            }

            // Force the next snapshot apply to rebuild from the remaining fresh
            // messages. This also prevents an old Agent snapshot from resurrecting
            // expired rows because ApplyVoiceChat uses each message timestamp.
            voiceChatSignature = "expired:" + now;
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
                Duration = TimeSpan.FromMilliseconds(110),
                EasingFunction = easing,
                EnableDependentAnimation = true,
            };
            Storyboard.SetTarget(slide, translate);
            Storyboard.SetTargetProperty(slide, "Y");

            var fade = new DoubleAnimation
            {
                From = 0,
                To = 1,
                Duration = TimeSpan.FromMilliseconds(90),
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
                Duration = TimeSpan.FromMilliseconds(110),
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
            var messageId = item.GetNamedNumber("message_id", 0);
            if (messageId > 0)
                return "id:" + messageId.ToString("0", System.Globalization.CultureInfo.InvariantCulture);
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

        private bool IsShiftDown()
        {
            return (Window.Current.CoreWindow.GetKeyState(VirtualKey.Shift) & CoreVirtualKeyStates.Down) != 0;
        }

        private void RefreshYoutubeDragSurface()
        {
            if (draggedPanel != YoutubePanel)
                YoutubeDragSurface.IsHitTestVisible = IsShiftDown();
        }

        private void YoutubeDragSurface_PointerPressed(object sender, PointerRoutedEventArgs e)
        {
            if (!IsShiftDown() || panelResizePointerId.HasValue)
            {
                YoutubeDragSurface.IsHitTestVisible = false;
                return;
            }
            BeginPanelDrag(YoutubePanel, e, YoutubeDragSurface);
        }

        private void YoutubeDragSurface_PointerMoved(object sender, PointerRoutedEventArgs e)
        {
            ContinuePanelDrag(e, YoutubeDragSurface);
            RefreshYoutubeDragSurface();
        }

        private void YoutubeDragSurface_PointerReleased(object sender, PointerRoutedEventArgs e)
        {
            EndPanelDrag(e, YoutubeDragSurface, true);
            RefreshYoutubeDragSurface();
        }

        private void YoutubeDragSurface_PointerCanceled(object sender, PointerRoutedEventArgs e)
        {
            EndPanelDrag(e, YoutubeDragSurface, false);
            RefreshYoutubeDragSurface();
        }

        private void YoutubeDragSurface_PointerCaptureLost(object sender, PointerRoutedEventArgs e)
        {
            if (panelDragPointerId == e.Pointer.PointerId) ClearPanelDrag();
            RefreshYoutubeDragSurface();
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
                // Panels are the default drag surface. Only actual controls keep
                // their own hitbox so padding/background around them still drags.
                if (current is Button || current is TextBox || current is ToggleSwitch ||
                    current is Slider || current is ComboBox || current is WebView)
                    return true;
                if (current == YoutubeVideoHost || current == BrowserWebHost || current == RootGrid) break;
            }
            return source != null && (IsDescendantOf(source, YoutubeVideoHost) || IsDescendantOf(source, BrowserWebHost));
        }

        private static bool IsDescendantOf(DependencyObject source, DependencyObject ancestor)
        {
            if (source == null || ancestor == null) return false;
            for (var current = source; current != null; current = VisualTreeHelper.GetParent(current))
            {
                if (current == ancestor) return true;
            }
            return false;
        }

        private void RefreshWinGPanelSettings()
        {
            var editing = widget != null && widget.GameBarDisplayMode != XboxGameBarDisplayMode.PinnedOnly;
            WinGPanelSettings.Visibility = editing ? Visibility.Visible : Visibility.Collapsed;
            if (!editing) return;

            winGPanelSettingsLoading = true;
            try
            {
                WinGLcuToggle.IsOn = HudPreferences.LcuPanelVisible && !HudPreferences.LcuPanelClosed;
                WinGPartyToggle.IsOn = HudPreferences.PartyPanelVisible && !HudPreferences.PartyPanelClosed;
                WinGYoutubeToggle.IsOn = YoutubePreferences.Visible && !HudPreferences.YoutubePanelClosed;
                WinGBrowserToggle.IsOn = HudPreferences.BrowserPanelVisible && !HudPreferences.BrowserPanelClosed;
                WinGNativeInputToggle.IsOn = HudPreferences.NativeInputPanelVisible && !HudPreferences.NativeInputPanelClosed;
                WinGVoiceChatToggle.IsOn = HudPreferences.VoiceChatPanelVisible && !HudPreferences.VoiceChatPanelClosed;
            }
            finally
            {
                winGPanelSettingsLoading = false;
            }
        }

        private void WinGPanelToggle_Toggled(object sender, RoutedEventArgs e)
        {
            if (winGPanelSettingsLoading) return;
            var toggle = sender as ToggleSwitch;
            var key = toggle?.Tag as string;
            if (string.IsNullOrWhiteSpace(key)) return;

            var visible = toggle.IsOn;
            if (visible)
            {
                SetPanelClosed(key, false);
                HudPreferences.Visible = true;
            }
            SetPanelVisible(key, visible);
            ApplyPreferences();
            App.LogResize("wing-panel-toggle key=" + key + " visible=" + visible);
        }

        private void PanelEyeButton_Click(object sender, RoutedEventArgs e)
        {
            var key = (sender as FrameworkElement)?.Tag as string;
            if (string.IsNullOrWhiteSpace(key)) return;

            // The eye controls runtime visibility, not layout expansion. In Win+G
            // hidden panels stay present as a 60% ghost so the user can immediately
            // click the eye again; only PinnedOnly/the real game overlay collapses them.
            var visible = !GetPanelVisible(key);
            SetPanelVisible(key, visible);
            if (visible) HudPreferences.Visible = true;
            ApplyPreferences();
            App.LogResize("panel-eye-toggle key=" + key + " visible=" + visible +
                " mode=" + (widget != null ? widget.GameBarDisplayMode.ToString() : "none"));
        }

        private void PanelCloseButton_Click(object sender, RoutedEventArgs e)
        {
            var key = (sender as FrameworkElement)?.Tag as string;
            if (string.IsNullOrWhiteSpace(key)) return;
            SetPanelClosed(key, true);
            SetPanelVisible(key, false);
            UpdatePanelChrome();
            RefreshWinGPanelSettings();
            App.LogResize("panel-close key=" + key +
                " mode=" + (widget != null ? widget.GameBarDisplayMode.ToString() : "none"));
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
            RestorePanelPosition(BrowserPanel, BrowserPanelTransform, BrowserPanelXSetting, BrowserPanelYSetting);
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
            RestorePanelSize("browser", values, BrowserPanelWidthSetting, BrowserPanelHeightSetting, 720, 420);
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
            else if (key == "browser") { minWidth = 360; maxWidth = 1600; minHeight = 240; maxHeight = 1000; }
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
            else if (key == "browser")
            {
                values[BrowserPanelWidthSetting] = panel.Width;
                values[BrowserPanelHeightSetting] = body.Height;
            }
            else if (key == "click-native") { values[ClickNativePanelWidthSetting] = panel.Width; values[ClickNativePanelHeightSetting] = GetPanelExpanded(key) ? body.Height : clickNativeExpandedHeight; }
        }

        private FrameworkElement GetPanel(string key)
        {
            if (key == "lcu") return LcuPanel;
            if (key == "party") return PartyPanel;
            if (key == "youtube") return YoutubePanel;
            if (key == "browser") return BrowserPanel;
            if (key == "click-native") return ClickNativePanel;
            if (key == "voice-chat") return VoiceChatPanel;
            return null;
        }

        private FrameworkElement GetPanelBody(string key)
        {
            if (key == "lcu") return LcuBody;
            if (key == "party") return PartyBody;
            if (key == "youtube") return YoutubeVideoHost;
            if (key == "browser") return BrowserBody;
            if (key == "click-native") return ClickNativeBody;
            return null;
        }

        private bool GetPanelVisible(string key)
        {
            if (key == "lcu") return HudPreferences.LcuPanelVisible;
            if (key == "party") return HudPreferences.PartyPanelVisible;
            if (key == "youtube") return YoutubePreferences.Visible;
            if (key == "browser") return HudPreferences.BrowserPanelVisible;
            if (key == "click-native") return HudPreferences.NativeInputPanelVisible;
            if (key == "voice-chat") return HudPreferences.VoiceChatPanelVisible;
            return false;
        }

        private void SetPanelVisible(string key, bool visible)
        {
            if (key == "lcu") HudPreferences.LcuPanelVisible = visible;
            else if (key == "party") HudPreferences.PartyPanelVisible = visible;
            else if (key == "youtube") YoutubePreferences.Visible = visible;
            else if (key == "browser") HudPreferences.BrowserPanelVisible = visible;
            else if (key == "click-native") HudPreferences.NativeInputPanelVisible = visible;
            else if (key == "voice-chat") HudPreferences.VoiceChatPanelVisible = visible;
        }

        private bool GetPanelClosed(string key)
        {
            if (key == "lcu") return HudPreferences.LcuPanelClosed;
            if (key == "party") return HudPreferences.PartyPanelClosed;
            if (key == "youtube") return HudPreferences.YoutubePanelClosed;
            if (key == "browser") return HudPreferences.BrowserPanelClosed;
            if (key == "click-native") return HudPreferences.NativeInputPanelClosed;
            if (key == "voice-chat") return HudPreferences.VoiceChatPanelClosed;
            return false;
        }

        private void SetPanelClosed(string key, bool closed)
        {
            if (key == "lcu") HudPreferences.LcuPanelClosed = closed;
            else if (key == "party") HudPreferences.PartyPanelClosed = closed;
            else if (key == "youtube") HudPreferences.YoutubePanelClosed = closed;
            else if (key == "browser") HudPreferences.BrowserPanelClosed = closed;
            else if (key == "click-native") HudPreferences.NativeInputPanelClosed = closed;
            else if (key == "voice-chat") HudPreferences.VoiceChatPanelClosed = closed;
        }

        private bool GetPanelExpanded(string key)
        {
            if (key == "lcu") return HudPreferences.LcuPanelExpanded;
            if (key == "party") return HudPreferences.PartyPanelExpanded;
            if (key == "youtube") return HudPreferences.YoutubePanelExpanded;
            if (key == "browser") return HudPreferences.BrowserPanelExpanded;
            if (key == "click-native") return HudPreferences.NativeInputPanelExpanded;
            return true;
        }

        private void SetPanelExpanded(string key, bool expanded)
        {
            if (key == "lcu") HudPreferences.LcuPanelExpanded = expanded;
            else if (key == "party") HudPreferences.PartyPanelExpanded = expanded;
            else if (key == "youtube") HudPreferences.YoutubePanelExpanded = expanded;
            else if (key == "browser") HudPreferences.BrowserPanelExpanded = expanded;
            else if (key == "click-native") HudPreferences.NativeInputPanelExpanded = expanded;
        }

        private void UpdatePanelChrome()
        {
            bool pinnedOnly = widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly;
            ApplyPinnedBodyPositionOffset(pinnedOnly);
            ApplyPanelChrome("lcu", LcuPanel, LcuMoveBar, LcuBody, LcuEyeButton, LcuResizeHandles, pinnedOnly);
            ApplyPanelChrome("party", PartyPanel, PartyMoveBar, PartyBody, PartyEyeButton, PartyResizeHandles, pinnedOnly);
            ApplyPanelChrome("youtube", YoutubePanel, YoutubeMoveBar, YoutubeVideoHost, YoutubeEyeButton, YoutubeResizeHandles, pinnedOnly);
            ApplyPanelChrome("browser", BrowserPanel, BrowserMoveBar, BrowserBody, BrowserEyeButton, BrowserResizeHandles, pinnedOnly);
            ApplyBrowserRuntimeVisibility(pinnedOnly);
            ApplyClickTestPanelChrome(pinnedOnly);
            UpdateVoiceChatPanelChrome(pinnedOnly);
            ApplyYoutubeRuntimeVisibility(pinnedOnly);
            UpdateNativeInputDiagnosticsMode(false);
        }

        private void ApplyClickTestPanelChrome(bool pinnedOnly)
        {
            var runtimeVisible = HudPreferences.NativeInputPanelVisible;
            var expanded = HudPreferences.NativeInputPanelExpanded;
            var hidden = !runtimeVisible;
            var closed = HudPreferences.NativeInputPanelClosed;

            // Eye-hidden: 60% ghost in Win+G, invisible in the real overlay.
            // X-closed: completely absent in both modes without using the old
            // expanded/collapsed state as a close mechanism.
            ClickNativePanel.Visibility = Visibility.Visible;
            ClickNativePanel.Opacity = closed ? 0.0 : (!pinnedOnly && hidden ? HiddenRuntimePanelOpacity : (pinnedOnly && hidden ? 0.0 : 1.0));
            ClickNativePanel.IsHitTestVisible = !closed && !pinnedOnly;
            ClickNativeMoveBar.Visibility = !pinnedOnly ? Visibility.Visible : Visibility.Collapsed;
            ClickNativeBody.Visibility = Visibility.Visible;
            ClickNativeDetails.Visibility = expanded ? Visibility.Visible : Visibility.Collapsed;
            ClickNativeEyeButton.Opacity = runtimeVisible ? 1.0 : 0.5;
            ToolTipService.SetToolTip(ClickNativeEyeButton, runtimeVisible ?
                "실제 화면에서 숨기기" : "실제 화면에 표시");
            ClickNativeResizeHandles.Visibility = expanded && !pinnedOnly ? Visibility.Visible : Visibility.Collapsed;

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
            var runtimeVisible = HudPreferences.VoiceChatPanelVisible;
            var hidden = !runtimeVisible;
            var closed = HudPreferences.VoiceChatPanelClosed;
            var showRuntimeContent = runtimeVisible && voiceChatHasVisibleMessages;

            // Match the other panels: eye-hidden stays as a 60% Win+G ghost but
            // disappears from the real overlay; X-closed is absent from both modes.
            VoiceChatPanel.Visibility = editing || showRuntimeContent ? Visibility.Visible : Visibility.Collapsed;
            VoiceChatPanel.Opacity = closed ? 0.0 : (!pinnedOnly && hidden ? HiddenRuntimePanelOpacity : 1.0);
            VoiceChatPanel.IsHitTestVisible = !closed && !pinnedOnly;
            VoiceChatMoveBar.Visibility = editing ? Visibility.Visible : Visibility.Collapsed;
            VoiceChatBody.Visibility = editing || showRuntimeContent ? Visibility.Visible : Visibility.Collapsed;
            VoiceChatOverlay.Visibility = voiceChatHasVisibleMessages ? Visibility.Visible : Visibility.Collapsed;
            VoiceChatPlaceholder.Visibility = editing && !voiceChatHasVisibleMessages ? Visibility.Visible : Visibility.Collapsed;
            VoiceChatEyeButton.Opacity = runtimeVisible ? 1.0 : 0.5;
            ToolTipService.SetToolTip(VoiceChatEyeButton, runtimeVisible ?
                "실제 화면에서 숨기기" : "실제 화면에 표시");
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
            BrowserPanelTransform.Y += delta;
            ClickNativePanelTransform.Y += delta;
            VoiceChatPanelTransform.Y += delta;
            panelChromePinnedOffsetApplied = pinnedOnly;
        }

        private void ApplyPanelChrome(string key, FrameworkElement panel, FrameworkElement moveBar, FrameworkElement body, Control eyeButton,
            FrameworkElement resizeHandles, bool pinnedOnly)
        {
            bool runtimeVisible = GetPanelVisible(key);
            bool expanded = GetPanelExpanded(key);
            bool hidden = !runtimeVisible;
            bool closed = GetPanelClosed(key);

            // Eye-hidden: 60% ghost in Win+G, invisible in the real overlay.
            // X-closed: fully absent from both modes, independently of expanded state.
            panel.Visibility = Visibility.Visible;
            panel.Opacity = closed ? 0.0 : (!pinnedOnly && hidden ? HiddenRuntimePanelOpacity : (pinnedOnly && hidden ? 0.0 : 1.0));
            panel.IsHitTestVisible = !closed && !pinnedOnly;
            moveBar.Visibility = !pinnedOnly ? Visibility.Visible : Visibility.Collapsed;
            body.Visibility = pinnedOnly ? Visibility.Visible : (expanded ? Visibility.Visible : Visibility.Collapsed);
            eyeButton.Opacity = runtimeVisible ? 1.0 : 0.5;
            ToolTipService.SetToolTip(eyeButton, runtimeVisible ?
                "실제 화면에서 숨기기" : "실제 화면에 표시");
            resizeHandles.Visibility = expanded && !pinnedOnly ? Visibility.Visible : Visibility.Collapsed;
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
            // PinnedOnly hides the 24-DIP edit title bar and offsets every panel by
            // the same amount so the body stays visually fixed. Persist edit-space Y
            // even when a native runtime drag finishes while the title bar is hidden.
            var storedY = transform.Y - (panelChromePinnedOffsetApplied ? PanelTitleBarHeight : 0.0);
            if (panel == LcuPanel)
            {
                values[LcuPanelXSetting] = transform.X;
                values[LcuPanelYSetting] = storedY;
            }
            else if (panel == PartyPanel)
            {
                values[PartyPanelXSetting] = transform.X;
                values[PartyPanelYSetting] = storedY;
            }
            else if (panel == YoutubePanel)
            {
                values[YoutubePanelXSetting] = transform.X;
                values[YoutubePanelYSetting] = storedY;
            }
            else if (panel == BrowserPanel)
            {
                values[BrowserPanelXSetting] = transform.X;
                values[BrowserPanelYSetting] = storedY;
            }
            else if (panel == ClickNativePanel) { values[ClickNativePanelXSetting] = transform.X; values[ClickNativePanelYSetting] = storedY; }
            else if (panel == VoiceChatPanel) { values[VoiceChatPanelXSetting] = transform.X; values[VoiceChatPanelYSetting] = storedY; }
        }

        private void ClampAllPanelPositions()
        {
            SetPanelPosition(LcuPanel, LcuPanelTransform, LcuPanelTransform.X, LcuPanelTransform.Y, false);
            SetPanelPosition(PartyPanel, PartyPanelTransform, PartyPanelTransform.X, PartyPanelTransform.Y, false);
            SetPanelPosition(YoutubePanel, YoutubePanelTransform, YoutubePanelTransform.X, YoutubePanelTransform.Y, false);
            SetPanelPosition(BrowserPanel, BrowserPanelTransform, BrowserPanelTransform.X, BrowserPanelTransform.Y, false);
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

        private void AddNativeInputBlockPanel(JsonArray rects, FrameworkElement panel)
        {
            if (panel == null || panel.Visibility != Visibility.Visible || panel.Opacity <= 0.01) return;
            var width = panel.ActualWidth > 0 ? panel.ActualWidth : (!double.IsNaN(panel.Width) ? panel.Width : 0.0);
            var height = panel.ActualHeight > 0 ? panel.ActualHeight : (!double.IsNaN(panel.Height) ? panel.Height : 0.0);
            if (width <= 0 || height <= 0) return;
            Windows.Foundation.Point local;
            try { local = panel.TransformToVisual(RootGrid).TransformPoint(new Windows.Foundation.Point(0, 0)); }
            catch { return; }
            AddNativeInputBlockRect(rects, local, width, height);
        }

        private void AddNativeInputBlockElement(JsonArray rects, FrameworkElement element)
        {
            if (element == null || element.Visibility != Visibility.Visible || element.Opacity <= 0.01 || !element.IsHitTestVisible) return;
            var width = element.ActualWidth > 0 ? element.ActualWidth : (!double.IsNaN(element.Width) ? element.Width : 0.0);
            var height = element.ActualHeight > 0 ? element.ActualHeight : (!double.IsNaN(element.Height) ? element.Height : 0.0);
            if (width <= 0 || height <= 0) return;
            Windows.Foundation.Point local;
            try { local = element.TransformToVisual(RootGrid).TransformPoint(new Windows.Foundation.Point(0, 0)); }
            catch { return; }
            AddNativeInputBlockRect(rects, local, width, height);
        }

        private void AddNativeInputBlockRect(JsonArray rects, Windows.Foundation.Point local, double width, double height)
        {
            if (widget == null || rects == null || width <= 0 || height <= 0) return;
            var bounds = widget.WindowBounds;
            var scale = Math.Max(1.0, DisplayInformation.GetForCurrentView().RawPixelsPerViewPixel);
            var rect = new JsonArray
            {
                JsonValue.CreateNumberValue((bounds.X + local.X) * scale),
                JsonValue.CreateNumberValue((bounds.Y + local.Y) * scale),
                JsonValue.CreateNumberValue(width * scale),
                JsonValue.CreateNumberValue(height * scale),
                JsonValue.CreateNumberValue(bounds.X + (local.X * scale)),
                JsonValue.CreateNumberValue(bounds.Y + (local.Y * scale)),
                JsonValue.CreateNumberValue(width * scale),
                JsonValue.CreateNumberValue(height * scale),
            };
            rects.Add(rect);
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

        private void UpdateNativeInputDiagnosticsMode(bool forceRefresh)
        {
            var pinnedOnly = widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly;
            // Native input is also the runtime drag transport. Keep it alive while
            // the HUD is visible even when the diagnostics panel itself is hidden.
            var wanted = widget != null && HudPreferences.Visible;
            nativeInputRuntimeDragWanted = wanted && pinnedOnly;
            var changed = nativeInputDiagnosticsWanted != wanted;
            nativeInputDiagnosticsWanted = wanted;
            if (wanted)
            {
                StartNativeInputLoop();
                if (changed || forceRefresh) _ = SyncExternalClickTestAsync(true);
            }
            else if (changed || forceRefresh)
            {
                _ = SyncExternalClickTestAsync(false);
            }
        }

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
                    if (!nativeInputDiagnosticsWanted)
                    {
                        if (!nativeInputDeactivatedWhileIdle)
                        {
                            nativeInputDeactivatedWhileIdle = true;
                            try
                            {
                                await Dispatcher.RunAsync(CoreDispatcherPriority.Low, async () =>
                                {
                                    try { await SyncExternalClickTestAsync(false); } catch { }
                                });
                            }
                            catch { }
                        }
                        try { await Task.Delay(1000, token); }
                        catch (TaskCanceledException) { break; }
                        continue;
                    }
                    nativeInputDeactivatedWhileIdle = false;
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
                            // Keep at most one UI probe queued. While a pinned panel is
                            // actively being dragged, prioritize cursor sampling so a busy
                            // Game Bar UI thread does not introduce visible drag stalls.
                            var dispatchPriority = Volatile.Read(ref nativePanelDragActive) != 0
                                ? CoreDispatcherPriority.High
                                : CoreDispatcherPriority.Low;
                            _ = Dispatcher.RunAsync(dispatchPriority, async () =>
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

                    var dragActive = Volatile.Read(ref nativePanelDragActive) != 0;
                    try { await Task.Delay(dragActive ? 16 : (nativeInputRuntimeDragWanted ? 33 : 100), token); }
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
            UpdateNativeInputDiagnosticsMode(true);
            UpdatePinTutorial();
            if (widget != null)
                RequestGameBarUiaProbeIfNeeded(widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly);
            QueueTutorialStateRefresh("window-activated-" + e.WindowActivationState);
            QueueForegroundChromeRefresh("window-activated-" + e.WindowActivationState);
        }

        private static bool PointInRect(double x, double y, double rx, double ry, double width, double height)
        {
            return width > 0 && height > 0 && x >= rx && x < rx + width && y >= ry && y < ry + height;
        }

        private void UpdateNativeHookButtonVisual(bool hovered, bool pressed)
        {
            if (NativeHookHitbox == null || NativeHookButtonTransform == null) return;

            if (pressed)
            {
                NativeHookHitbox.Background = new SolidColorBrush(Color.FromArgb(255, 65, 43, 86));
                NativeHookHitbox.BorderBrush = new SolidColorBrush(Colors.White);
                NativeHookButtonTransform.ScaleX = 0.97;
                NativeHookButtonTransform.ScaleY = 0.97;
                NativeHookButtonTransform.TranslateY = 1.5;
            }
            else if (hovered)
            {
                NativeHookHitbox.Background = new SolidColorBrush(Color.FromArgb(255, 114, 81, 151));
                NativeHookHitbox.BorderBrush = new SolidColorBrush(Color.FromArgb(220, 182, 154, 216));
                NativeHookButtonTransform.ScaleX = 1.015;
                NativeHookButtonTransform.ScaleY = 1.015;
                NativeHookButtonTransform.TranslateY = -1.0;
            }
            else
            {
                NativeHookHitbox.Background = new SolidColorBrush(Color.FromArgb(255, 91, 62, 120));
                NativeHookHitbox.BorderBrush = new SolidColorBrush(Color.FromArgb(128, 111, 85, 164));
                NativeHookButtonTransform.ScaleX = 1.0;
                NativeHookButtonTransform.ScaleY = 1.0;
                NativeHookButtonTransform.TranslateY = 0.0;
            }
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
                UpdateNativeHookButtonVisual(false, false);
                ClickNativeResult.Text = "Agent native input unavailable · widget=null";
                return;
            }
            syncingNativeInput = true;
            syncingNativeInputSinceMs = NowMs();
            var syncGeneration = Interlocked.Increment(ref nativeInputSyncGeneration);
            try
            {
                var requestedActive = active && HudPreferences.Visible && HudPanel.Visibility == Visibility.Visible;
                var diagnosticRequested = requestedActive
                    && ClickNativePanel.Visibility == Visibility.Visible
                    && !HudPreferences.NativeInputPanelClosed && ClickNativePanel.Opacity > 0.0;
                var geometryReady = NativeHookHitbox.ActualWidth > 0 && NativeHookHitbox.ActualHeight > 0;
                // The Agent must keep polling the raw cursor/button state in PinnedOnly
                // even when the diagnostic button is hidden. Use an off-screen 1x1
                // hitbox in that case so click counting stays disabled.
                var effectiveActive = requestedActive;
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
                var pinnedOnly = widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly;
                var inputBlockRects = new JsonArray();
                if (effectiveActive && pinnedOnly)
                {
                    AddNativeInputBlockPanel(inputBlockRects, LcuPanel);
                    AddNativeInputBlockPanel(inputBlockRects, PartyPanel);
                    AddNativeInputBlockPanel(inputBlockRects, YoutubePanel);
                    AddNativeInputBlockPanel(inputBlockRects, BrowserPanel);
                    AddNativeInputBlockPanel(inputBlockRects, ClickNativePanel);
                    AddNativeInputBlockPanel(inputBlockRects, VoiceChatPanel);
                    // These runtime controls receive their clicks through the Agent while
                    // Game Bar itself remains click-through. Swallow the original game
                    // click and forward only the overlay action.
                    AddNativeInputBlockElement(inputBlockRects, NativeHookHitbox);
                    if (!youtubeUsingFallback && youtubeWebView2 != null && youtubeWebView2.Visibility == Visibility.Visible)
                        AddNativeInputBlockElement(inputBlockRects, YoutubeVideoHost);
                }
                command["blockInput"] = JsonValue.CreateBooleanValue(inputBlockRects.Count > 0);
                command["blockRects"] = inputBlockRects;
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
                    if (diagnosticRequested && geometryReady)
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
                    else
                    {
                        command["x"] = JsonValue.CreateNumberValue(-99999);
                        command["y"] = JsonValue.CreateNumberValue(-99999);
                        command["width"] = JsonValue.CreateNumberValue(1);
                        command["height"] = JsonValue.CreateNumberValue(1);
                        command["altX"] = JsonValue.CreateNumberValue(-99999);
                        command["altY"] = JsonValue.CreateNumberValue(-99999);
                        command["altWidth"] = JsonValue.CreateNumberValue(1);
                        command["altHeight"] = JsonValue.CreateNumberValue(1);
                    }
                }

                var payload = command.Stringify();
                lastNativeInputKeepalivePayload = payload;
                var rawResponse = await SendAgentCommandAsync(payload);
                lastNativeInputSyncOkAtMs = NowMs();
                var response = JsonObject.Parse(rawResponse);
                if (!response.GetNamedBoolean("ok", false))
                {
                    UpdateNativeHookButtonVisual(false, false);
                    ClickNativeResult.Text = "Agent native · " + response.GetNamedString("message", "sync failed");
                    return;
                }
                var sequence = (ulong)Math.Max(0, response.GetNamedNumber("sequence", 0));
                var clickCount = (ulong)Math.Max(0, response.GetNamedNumber("clickCount", 0));
                if (diagnosticRequested) NativeHookClickCountText.Text = clickCount.ToString();
                var hudToggleSequence = (ulong)Math.Max(0, response.GetNamedNumber("hudToggleSequence", 0));
                if (!hudToggleSequenceInitialized)
                {
                    lastHudToggleSequence = hudToggleSequence;
                    hudToggleSequenceInitialized = true;
                }
                else if (hudToggleSequence != lastHudToggleSequence)
                {
                    lastHudToggleSequence = hudToggleSequence;
                    ToggleRuntimePanelsVisibility("agent-native");
                    App.LogStage("hud-toggle-f9-runtime-panels");
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
                var inputBlockActive = response.GetNamedBoolean("inputBlockActive", false);
                var inputBlockCaptured = response.GetNamedBoolean("inputBlockCaptured", false);
                var inputBlockedDownCount = (ulong)Math.Max(0, response.GetNamedNumber("inputBlockedDownCount", 0));
                var inputBlockedUpCount = (ulong)Math.Max(0, response.GetNamedNumber("inputBlockedUpCount", 0));
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
                var remotePointerVisible = response.GetNamedBoolean("remotePointerVisible", false);
                var remotePointerX = Math.Max(0.0, Math.Min(1.0, response.GetNamedNumber("remotePointerX", 0.5)));
                var remotePointerY = Math.Max(0.0, Math.Min(1.0, response.GetNamedNumber("remotePointerY", 0.5)));

                var fallbackHovered =
                    PointInRect(cursorX, cursorY, rectX, rectY, rectWidth, rectHeight) ||
                    PointInRect(cursorX, cursorY, altRectX, altRectY, altRectWidth, altRectHeight) ||
                    PointInRect(cursorX, cursorY, rectX + monitorX, rectY + monitorY, rectWidth, rectHeight) ||
                    PointInRect(cursorX, cursorY, altRectX + monitorX, altRectY + monitorY, altRectWidth, altRectHeight);
                var nativeHovered = response.ContainsKey("hovered")
                    ? response.GetNamedBoolean("hovered", fallbackHovered)
                    : fallbackHovered;
                var nativeMouseDown = response.ContainsKey("mouseDown")
                    ? response.GetNamedBoolean("mouseDown", false)
                    : nativeHovered && eventName == "down";
                var leftButtonDown = response.ContainsKey("leftButtonDown")
                    ? response.GetNamedBoolean("leftButtonDown", false)
                    : nativeMouseDown;
                if (diagnosticRequested) UpdateNativeHookButtonVisual(nativeHovered, nativeMouseDown);
                HandlePinnedNativePanelDrag(leftButtonDown, cursorX, cursorY, monitorX, monitorY);
                await HandlePinnedYoutubeNativeInputAsync(leftButtonDown, cursorX, cursorY, monitorX, monitorY);

                if (sequence != lastNativeInputSequence) lastNativeInputSequence = sequence;

                if (diagnosticRequested)
                {
                    UpdateCursorDiagnosticMarkers(
                        physicalX, physicalY, logicalX, logicalY, cursorInfoX, cursorInfoY, cursorInfoValid,
                        gamebarPrimaryX, gamebarPrimaryY, gamebarAltX, gamebarAltY, cursorX, cursorY);
                }
                UpdateRemotePointerMarker(remotePointerVisible, remotePointerX, remotePointerY);

                if (diagnosticRequested) ClickNativeResult.Text =
                    "transport=" + lastAgentCommandTransport + " pipeErr=" + lastAgentPipeError + " tcpErr=" + lastAgentTcpError +
                    " requestedActive=" + requestedActive + " geometryReady=" + geometryReady +
                    " hitbox=" + NativeHookHitbox.ActualWidth.ToString("0.##") + "x" + NativeHookHitbox.ActualHeight.ToString("0.##") + "\n" +
                    "agent pid=" + agentPid + " build=" + agentBuild + " release=" + releaseLabel + " commit=" + agentCommit + "\n" +
                    "active=" + responseActive + " seq=" + sequence + " clicks=" + clickCount + " event=" + eventName +
                    " eventXY=" + eventX + "," + eventY + " eventAt=" + eventAtMs + " matched=" + matched + "\n" +
                    "inputBlock active=" + inputBlockActive + " captured=" + inputBlockCaptured +
                    " down=" + inputBlockedDownCount + " up=" + inputBlockedUpCount + " rects=" + inputBlockRects.Count + "\n" +
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
                UpdateNativeHookButtonVisual(false, false);
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

        private void HandlePinnedNativePanelDrag(bool leftButtonDown, int cursorX, int cursorY, int monitorX, int monitorY)
        {
            var pinnedOnly = widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly;
            if (!pinnedOnly || !HudPreferences.Visible || HudPanel.Visibility != Visibility.Visible)
            {
                if (nativeDraggedPanel != null) EndPinnedNativePanelDrag(true);
                nativePanelMouseWasDown = leftButtonDown;
                return;
            }

            if (leftButtonDown && !nativePanelMouseWasDown)
                TryBeginPinnedNativePanelDrag(cursorX, cursorY, monitorX, monitorY);

            if (leftButtonDown && nativeDraggedPanel != null && nativeDraggedPanelTransform != null)
            {
                var point = NativeCursorToRoot(cursorX, cursorY, monitorX, monitorY, nativePanelDragCoordinateMode);
                var dx = point.X - nativePanelDragStartPoint.X;
                var dy = point.Y - nativePanelDragStartPoint.Y;
                if (!nativePanelDragMoved && Math.Sqrt((dx * dx) + (dy * dy)) >= RuntimeNativeDragThreshold)
                    nativePanelDragMoved = true;
                if (nativePanelDragMoved)
                {
                    SetPanelPosition(
                        nativeDraggedPanel,
                        nativeDraggedPanelTransform,
                        nativePanelDragStartX + dx,
                        nativePanelDragStartY + dy,
                        false);
                }
            }
            else if (!leftButtonDown && nativePanelMouseWasDown && nativeDraggedPanel != null)
            {
                EndPinnedNativePanelDrag(true);
            }

            nativePanelMouseWasDown = leftButtonDown;
        }

        private void TryBeginPinnedNativePanelDrag(int cursorX, int cursorY, int monitorX, int monitorY)
        {
            for (var mode = 0; mode < 4; mode++)
            {
                var point = NativeCursorToRoot(cursorX, cursorY, monitorX, monitorY, mode);
                var panel = FindPinnedNativeDragPanel(point);
                if (panel == null) continue;
                var transform = panel.RenderTransform as TranslateTransform;
                if (transform == null) return;
                nativeDraggedPanel = panel;
                nativeDraggedPanelTransform = transform;
                nativePanelDragStartPoint = point;
                nativePanelDragStartX = transform.X;
                nativePanelDragStartY = transform.Y;
                nativePanelDragCoordinateMode = mode;
                nativePanelDragMoved = false;
                Interlocked.Exchange(ref nativePanelDragActive, 1);
                App.LogStage("runtime-native-drag-start panel=" + NativePanelName(panel) + " mode=" + mode);
                return;
            }
        }

        private FrameworkElement FindPinnedNativeDragPanel(Windows.Foundation.Point point)
        {
            // Later/high-Z panels win when their runtime drag bands overlap.
            var panels = new[] { VoiceChatPanel, BrowserPanel, YoutubePanel, ClickNativePanel, PartyPanel, LcuPanel };
            foreach (var panel in panels)
            {
                if (PinnedNativeDragHit(panel, point)) return panel;
            }
            return null;
        }

        private bool PinnedNativeDragHit(FrameworkElement panel, Windows.Foundation.Point point)
        {
            if (panel == null || panel.Visibility != Visibility.Visible || panel.Opacity <= 0.01) return false;
            var width = panel.ActualWidth > 0 ? panel.ActualWidth : (!double.IsNaN(panel.Width) ? panel.Width : 0.0);
            var height = panel.ActualHeight > 0 ? panel.ActualHeight : (!double.IsNaN(panel.Height) ? panel.Height : 0.0);
            if (width <= 0 || height <= 0) return false;
            Windows.Foundation.Point origin;
            try { origin = panel.TransformToVisual(RootGrid).TransformPoint(new Windows.Foundation.Point(0, 0)); }
            catch { return false; }
            var inside = point.X >= origin.X && point.X < origin.X + width
                && point.Y >= origin.Y && point.Y < origin.Y + height;
            if (!inside) return false;

            // The full outer card is draggable. Native interactive surfaces keep
            // only their own exact rectangle as a non-drag hitbox.
            if (panel == ClickNativePanel && PointInsideRootElement(NativeHookHitbox, point)) return false;
            if (panel == YoutubePanel && PointInsideRootElement(YoutubeVideoHost, point)) return false;
            return true;
        }

        private bool PointInsideRootElement(FrameworkElement element, Windows.Foundation.Point point)
        {
            if (element == null || element.Visibility != Visibility.Visible || element.Opacity <= 0.01) return false;
            var width = element.ActualWidth > 0 ? element.ActualWidth : (!double.IsNaN(element.Width) ? element.Width : 0.0);
            var height = element.ActualHeight > 0 ? element.ActualHeight : (!double.IsNaN(element.Height) ? element.Height : 0.0);
            if (width <= 0 || height <= 0) return false;
            Windows.Foundation.Point origin;
            try { origin = element.TransformToVisual(RootGrid).TransformPoint(new Windows.Foundation.Point(0, 0)); }
            catch { return false; }
            return point.X >= origin.X && point.X < origin.X + width
                && point.Y >= origin.Y && point.Y < origin.Y + height;
        }

        private Windows.Foundation.Point NativeCursorToRoot(int cursorX, int cursorY, int monitorX, int monitorY, int mode)
        {
            if (widget == null) return new Windows.Foundation.Point(cursorX, cursorY);
            var bounds = widget.WindowBounds;
            var scale = Math.Max(1.0, DisplayInformation.GetForCurrentView().RawPixelsPerViewPixel);
            switch (mode)
            {
                case 1:
                    return new Windows.Foundation.Point((cursorX - bounds.X) / scale, (cursorY - bounds.Y) / scale);
                case 2:
                    return new Windows.Foundation.Point(((cursorX - monitorX) / scale) - bounds.X, ((cursorY - monitorY) / scale) - bounds.Y);
                case 3:
                    return new Windows.Foundation.Point((cursorX - monitorX - bounds.X) / scale, (cursorY - monitorY - bounds.Y) / scale);
                default:
                    return new Windows.Foundation.Point((cursorX / scale) - bounds.X, (cursorY / scale) - bounds.Y);
            }
        }

        private void EndPinnedNativePanelDrag(bool save)
        {
            if (save && nativePanelDragMoved && nativeDraggedPanel != null && nativeDraggedPanelTransform != null)
                SavePanelPosition(nativeDraggedPanel, nativeDraggedPanelTransform);
            if (nativeDraggedPanel != null)
                App.LogStage("runtime-native-drag-stop panel=" + NativePanelName(nativeDraggedPanel) + " moved=" + nativePanelDragMoved);
            nativeDraggedPanel = null;
            nativeDraggedPanelTransform = null;
            nativePanelDragCoordinateMode = -1;
            nativePanelDragMoved = false;
            Interlocked.Exchange(ref nativePanelDragActive, 0);
        }

        private string NativePanelName(FrameworkElement panel)
        {
            if (panel == LcuPanel) return "lcu";
            if (panel == PartyPanel) return "party";
            if (panel == YoutubePanel) return "youtube";
            if (panel == BrowserPanel) return "browser";
            if (panel == ClickNativePanel) return "click-native";
            if (panel == VoiceChatPanel) return "voice-chat";
            return "unknown";
        }

        private async Task HandlePinnedYoutubeNativeInputAsync(bool leftButtonDown, int cursorX, int cursorY, int monitorX, int monitorY)
        {
            var pinnedOnly = widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly;
            var available = pinnedOnly
                && HudPreferences.Visible
                && HudPanel.Visibility == Visibility.Visible
                && !HudPreferences.YoutubePanelClosed
                && YoutubePreferences.Visible
                && YoutubePanel.Visibility == Visibility.Visible
                && YoutubePanel.Opacity > 0.01
                && !youtubeUsingFallback
                && youtubeWebView2 != null
                && youtubeWebView2.CoreWebView2 != null
                && youtubeWebView2.Visibility == Visibility.Visible;

            if (!available)
            {
                if (youtubeNativePointerCaptured && youtubeNativeMouseWasDown)
                    await DispatchYoutubeMouseEventAsync("mouseReleased", youtubeNativeLastX, youtubeNativeLastY, false);
                ResetYoutubeNativePointerState(leftButtonDown);
                return;
            }

            Windows.Foundation.Point local;
            int coordinateMode;
            var inside = TryMapNativeCursorToElement(
                YoutubeVideoHost,
                cursorX,
                cursorY,
                monitorX,
                monitorY,
                out local,
                out coordinateMode);

            var pressed = leftButtonDown && !youtubeNativeMouseWasDown;
            var released = !leftButtonDown && youtubeNativeMouseWasDown;

            if (pressed && inside && nativeDraggedPanel == null)
            {
                youtubeNativePointerCaptured = true;
                youtubeNativeCoordinateMode = coordinateMode;
                youtubeNativeLastX = local.X;
                youtubeNativeLastY = local.Y;
                await DispatchYoutubeMouseEventAsync("mouseMoved", local.X, local.Y, true);
                await DispatchYoutubeMouseEventAsync("mousePressed", local.X, local.Y, true);
                App.LogStage("youtube-runtime-native-input-down mode=" + coordinateMode);
            }
            else if (youtubeNativePointerCaptured)
            {
                var capturedPoint = NativeCursorToRoot(cursorX, cursorY, monitorX, monitorY, youtubeNativeCoordinateMode);
                Windows.Foundation.Point origin;
                try
                {
                    origin = YoutubeVideoHost.TransformToVisual(RootGrid)
                        .TransformPoint(new Windows.Foundation.Point(0, 0));
                }
                catch
                {
                    origin = new Windows.Foundation.Point(capturedPoint.X, capturedPoint.Y);
                }
                var width = Math.Max(1.0, YoutubeVideoHost.ActualWidth);
                var height = Math.Max(1.0, YoutubeVideoHost.ActualHeight);
                local = new Windows.Foundation.Point(
                    Math.Max(0.0, Math.Min(width - 1.0, capturedPoint.X - origin.X)),
                    Math.Max(0.0, Math.Min(height - 1.0, capturedPoint.Y - origin.Y)));

                if (ShouldForwardYoutubeMouseMove(local.X, local.Y))
                    await DispatchYoutubeMouseEventAsync("mouseMoved", local.X, local.Y, leftButtonDown);

                youtubeNativeLastX = local.X;
                youtubeNativeLastY = local.Y;
                if (released)
                {
                    await DispatchYoutubeMouseEventAsync("mouseReleased", local.X, local.Y, false);
                    App.LogStage("youtube-runtime-native-input-up");
                    youtubeNativePointerCaptured = false;
                    youtubeNativeCoordinateMode = -1;
                }
            }
            else if (inside && !leftButtonDown && ShouldForwardYoutubeMouseMove(local.X, local.Y))
            {
                youtubeNativeCoordinateMode = coordinateMode;
                youtubeNativeLastX = local.X;
                youtubeNativeLastY = local.Y;
                await DispatchYoutubeMouseEventAsync("mouseMoved", local.X, local.Y, false);
            }

            youtubeNativeMouseWasDown = leftButtonDown;
        }

        private bool TryMapNativeCursorToElement(
            FrameworkElement element,
            int cursorX,
            int cursorY,
            int monitorX,
            int monitorY,
            out Windows.Foundation.Point local,
            out int coordinateMode)
        {
            local = new Windows.Foundation.Point();
            coordinateMode = -1;
            if (element == null || element.ActualWidth <= 0 || element.ActualHeight <= 0) return false;

            Windows.Foundation.Point origin;
            try
            {
                origin = element.TransformToVisual(RootGrid)
                    .TransformPoint(new Windows.Foundation.Point(0, 0));
            }
            catch { return false; }

            for (var mode = 0; mode < 4; mode++)
            {
                var root = NativeCursorToRoot(cursorX, cursorY, monitorX, monitorY, mode);
                var x = root.X - origin.X;
                var y = root.Y - origin.Y;
                if (x < 0 || y < 0 || x >= element.ActualWidth || y >= element.ActualHeight) continue;
                local = new Windows.Foundation.Point(x, y);
                coordinateMode = mode;
                return true;
            }
            return false;
        }

        private bool ShouldForwardYoutubeMouseMove(double x, double y)
        {
            var now = NowMs();
            var changed = double.IsNaN(youtubeNativeLastX)
                || Math.Abs(x - youtubeNativeLastX) >= 0.5
                || Math.Abs(y - youtubeNativeLastY) >= 0.5;
            if (!changed && now - youtubeNativeLastMoveAtMs < 250) return false;
            youtubeNativeLastMoveAtMs = now;
            youtubeNativeLastX = x;
            youtubeNativeLastY = y;
            return true;
        }

        private async Task DispatchYoutubeMouseEventAsync(string type, double x, double y, bool leftDown)
        {
            if (double.IsNaN(x) || double.IsNaN(y) || youtubeWebView2?.CoreWebView2 == null) return;
            try
            {
                var payload = new JsonObject
                {
                    ["type"] = JsonValue.CreateStringValue(type),
                    ["x"] = JsonValue.CreateNumberValue(Math.Max(0.0, x)),
                    ["y"] = JsonValue.CreateNumberValue(Math.Max(0.0, y)),
                    ["button"] = JsonValue.CreateStringValue(type == "mouseMoved" ? "none" : "left"),
                    ["buttons"] = JsonValue.CreateNumberValue(leftDown ? 1 : 0),
                    ["clickCount"] = JsonValue.CreateNumberValue(type == "mouseMoved" ? 0 : 1),
                    ["pointerType"] = JsonValue.CreateStringValue("mouse"),
                };
                await youtubeWebView2.CoreWebView2.CallDevToolsProtocolMethodAsync(
                    "Input.dispatchMouseEvent",
                    payload.Stringify());
            }
            catch (Exception ex)
            {
                var now = NowMs();
                if (now - youtubeNativeLastErrorLogAtMs >= 1000)
                {
                    youtubeNativeLastErrorLogAtMs = now;
                    App.LogStage("youtube-runtime-native-input-error-" + ErrorCode(ex));
                }
            }
        }

        private void ResetYoutubeNativePointerState(bool currentMouseDown)
        {
            youtubeNativePointerCaptured = false;
            youtubeNativeCoordinateMode = -1;
            youtubeNativeLastX = double.NaN;
            youtubeNativeLastY = double.NaN;
            youtubeNativeMouseWasDown = currentMouseDown;
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
            UpdateNativeInputDiagnosticsMode(visible);
        }

        private void UpdateCursorDiagnosticMarkers(
            int physicalX, int physicalY, int logicalX, int logicalY,
            int cursorInfoX, int cursorInfoY, bool cursorInfoValid,
            int gamebarPrimaryX, int gamebarPrimaryY, int gamebarAltX, int gamebarAltY,
            int selectedX, int selectedY)
        {
            if (widget == null || !HudPreferences.Visible || !HudPreferences.NativeCursorMarkersVisible ||
                (widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly && runtimeOverlayHiddenByHotkey)) return;
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

        private void UpdateRemotePointerMarker(bool visible, double normalizedX, double normalizedY)
        {
            if (RemotePointerOverlay == null || RemotePointerMarker == null || RootGrid == null) return;
            if (!visible || !HudPreferences.Visible ||
                (widget != null && widget.GameBarDisplayMode == XboxGameBarDisplayMode.PinnedOnly && runtimeOverlayHiddenByHotkey) ||
                RootGrid.ActualWidth <= 0 || RootGrid.ActualHeight <= 0)
            {
                RemotePointerMarker.Visibility = Visibility.Collapsed;
                return;
            }
            RemotePointerOverlay.Visibility = Visibility.Visible;
            var x = Math.Max(0.0, Math.Min(1.0, normalizedX)) * RootGrid.ActualWidth;
            var y = Math.Max(0.0, Math.Min(1.0, normalizedY)) * RootGrid.ActualHeight;
            Canvas.SetLeft(RemotePointerMarker, x - 11.0);
            Canvas.SetTop(RemotePointerMarker, y - 11.0);
            RemotePointerMarker.Visibility = Visibility.Visible;
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

        private void ApplyBrowserPreferences()
        {
            var desired = NormalizeBrowserInput(HudPreferences.BrowserUrl);
            if (!string.Equals(BrowserAddressBox.Text, desired, StringComparison.Ordinal))
                BrowserAddressBox.Text = desired;

            if (HudPreferences.BrowserPanelClosed || !HudPreferences.BrowserPanelVisible) return;
            if (!string.Equals(browserLoadedUrl, desired, StringComparison.OrdinalIgnoreCase))
                _ = NavigateBrowserAsync(desired, false);
        }

        private static string NormalizeBrowserInput(string input)
        {
            var value = (input ?? "").Trim();
            if (value.Length == 0) return "https://www.google.com/";

            Uri absolute;
            if (Uri.TryCreate(value, UriKind.Absolute, out absolute) &&
                (absolute.Scheme == Uri.UriSchemeHttp || absolute.Scheme == Uri.UriSchemeHttps))
                return absolute.AbsoluteUri;

            if (value.IndexOf(' ') < 0 && value.IndexOf('.') > 0)
            {
                var candidate = "https://" + value;
                if (Uri.TryCreate(candidate, UriKind.Absolute, out absolute)) return absolute.AbsoluteUri;
            }

            return "https://www.google.com/search?q=" + Uri.EscapeDataString(value);
        }

        private async void BrowserAddressBox_KeyDown(object sender, KeyRoutedEventArgs e)
        {
            if (e.Key != VirtualKey.Enter) return;
            e.Handled = true;
            await NavigateBrowserAsync(BrowserAddressBox.Text, true);
        }

        private void BrowserBackButton_Click(object sender, RoutedEventArgs e)
        {
            try
            {
                if (!browserUsingFallback && browserWebView2?.CoreWebView2 != null && browserWebView2.CoreWebView2.CanGoBack)
                    browserWebView2.CoreWebView2.GoBack();
                else if (browserUsingFallback && BrowserFallbackWebView.CanGoBack)
                    BrowserFallbackWebView.GoBack();
            }
            catch { }
        }

        private void BrowserForwardButton_Click(object sender, RoutedEventArgs e)
        {
            try
            {
                if (!browserUsingFallback && browserWebView2?.CoreWebView2 != null && browserWebView2.CoreWebView2.CanGoForward)
                    browserWebView2.CoreWebView2.GoForward();
                else if (browserUsingFallback && BrowserFallbackWebView.CanGoForward)
                    BrowserFallbackWebView.GoForward();
            }
            catch { }
        }

        private void BrowserReloadButton_Click(object sender, RoutedEventArgs e)
        {
            try
            {
                if (!browserUsingFallback && browserWebView2?.CoreWebView2 != null)
                    browserWebView2.CoreWebView2.Reload();
                else if (browserUsingFallback)
                    BrowserFallbackWebView.Refresh();
            }
            catch { }
        }

        private async Task NavigateBrowserAsync(string input, bool persist)
        {
            var url = NormalizeBrowserInput(input);
            if (persist && !string.Equals(HudPreferences.BrowserUrl, url, StringComparison.Ordinal))
                HudPreferences.BrowserUrl = url;
            BrowserAddressBox.Text = url;
            browserLoadedUrl = url;
            browserReady = false;
            BrowserStateText.Text = "페이지를 여는 중…";
            BrowserStateOverlay.Visibility = Visibility.Visible;

            if (await EnsureBrowserWebView2Async())
            {
                browserUsingFallback = false;
                BrowserFallbackWebView.Visibility = Visibility.Collapsed;
                browserWebView2.Visibility = Visibility.Visible;
                browserWebView2.Source = new Uri(url);
                return;
            }

            browserUsingFallback = true;
            BrowserFallbackWebView.Visibility = Visibility.Visible;
            try { BrowserFallbackWebView.Navigate(new Uri(url)); }
            catch
            {
                BrowserStateText.Text = "주소를 열 수 없습니다.";
                BrowserStateOverlay.Visibility = Visibility.Visible;
            }
        }

        private async Task<bool> EnsureBrowserWebView2Async()
        {
            if (browserWebView2Unavailable) return false;
            if (browserWebView2 != null && browserWebView2.CoreWebView2 != null) return true;
            try
            {
                if (browserWebView2 == null)
                {
                    EnsureWinUiResources();
                    browserWebView2 = new Microsoft.UI.Xaml.Controls.WebView2
                    {
                        HorizontalAlignment = HorizontalAlignment.Stretch,
                        VerticalAlignment = VerticalAlignment.Stretch
                    };
                    browserWebView2.NavigationCompleted += BrowserWebView2_NavigationCompleted;
                    BrowserWebHost.Children.Insert(0, browserWebView2);
                }
                await browserWebView2.EnsureCoreWebView2Async();
                if (browserWebView2.CoreWebView2 == null) throw new InvalidOperationException("CoreWebView2 is unavailable");
                browserWebView2.CoreWebView2.NavigationStarting -= BrowserCoreWebView2_NavigationStarting;
                browserWebView2.CoreWebView2.NavigationStarting += BrowserCoreWebView2_NavigationStarting;
                browserWebView2.CoreWebView2.Settings.AreDefaultContextMenusEnabled = true;
                browserWebView2.CoreWebView2.Settings.AreDevToolsEnabled = false;
                browserWebView2.CoreWebView2.Settings.IsStatusBarEnabled = false;
                return true;
            }
            catch (Exception ex)
            {
                browserWebView2Unavailable = true;
                browserUsingFallback = true;
                if (browserWebView2 != null) browserWebView2.Visibility = Visibility.Collapsed;
                App.LogStage("browser-webview2-fallback-" + ErrorCode(ex));
                return false;
            }
        }

        private void BrowserCoreWebView2_NavigationStarting(
            Microsoft.Web.WebView2.Core.CoreWebView2 sender,
            Microsoft.Web.WebView2.Core.CoreWebView2NavigationStartingEventArgs args)
        {
            if (string.IsNullOrWhiteSpace(args.Uri)) return;
            browserReady = false;
            browserLoadedUrl = args.Uri;
            BrowserAddressBox.Text = args.Uri;
            BrowserStateText.Text = "페이지를 여는 중…";
            BrowserStateOverlay.Visibility = Visibility.Visible;
        }

        private void BrowserWebView2_NavigationCompleted(
            Microsoft.UI.Xaml.Controls.WebView2 sender,
            Microsoft.Web.WebView2.Core.CoreWebView2NavigationCompletedEventArgs args)
        {
            if (!args.IsSuccess)
            {
                browserReady = false;
                BrowserStateText.Text = "페이지를 열지 못했습니다.";
                BrowserStateOverlay.Visibility = Visibility.Visible;
                return;
            }
            var source = sender.Source?.AbsoluteUri;
            if (!string.IsNullOrWhiteSpace(source))
            {
                browserLoadedUrl = source;
                BrowserAddressBox.Text = source;
                if (!string.Equals(HudPreferences.BrowserUrl, source, StringComparison.Ordinal))
                    HudPreferences.BrowserUrl = source;
            }
            browserReady = true;
            BrowserStateOverlay.Visibility = Visibility.Collapsed;
        }

        private void BrowserFallbackWebView_NavigationStarting(WebView sender, WebViewNavigationStartingEventArgs args)
        {
            if (args.Uri == null) return;
            browserReady = false;
            browserLoadedUrl = args.Uri.AbsoluteUri;
            BrowserAddressBox.Text = browserLoadedUrl;
            BrowserStateText.Text = "페이지를 여는 중…";
            BrowserStateOverlay.Visibility = Visibility.Visible;
        }

        private void BrowserFallbackWebView_NavigationCompleted(WebView sender, WebViewNavigationCompletedEventArgs args)
        {
            if (!args.IsSuccess)
            {
                browserReady = false;
                BrowserStateText.Text = "페이지를 열지 못했습니다.";
                BrowserStateOverlay.Visibility = Visibility.Visible;
                return;
            }
            if (args.Uri != null)
            {
                browserLoadedUrl = args.Uri.AbsoluteUri;
                BrowserAddressBox.Text = browserLoadedUrl;
                if (!string.Equals(HudPreferences.BrowserUrl, browserLoadedUrl, StringComparison.Ordinal))
                    HudPreferences.BrowserUrl = browserLoadedUrl;
            }
            browserReady = true;
            BrowserStateOverlay.Visibility = Visibility.Collapsed;
        }

        private void ApplyBrowserRuntimeVisibility(bool pinnedOnly)
        {
            var render = !HudPreferences.BrowserPanelClosed && HudPreferences.BrowserPanelVisible;
            if (browserWebView2 != null)
                browserWebView2.Visibility = render && !browserUsingFallback ? Visibility.Visible : Visibility.Collapsed;
            BrowserFallbackWebView.Visibility = render && browserUsingFallback ? Visibility.Visible : Visibility.Collapsed;

            if (!render && !HudPreferences.BrowserPanelClosed)
            {
                BrowserStateText.Text = "게임 화면에서 숨김";
                BrowserStateOverlay.Visibility = Visibility.Visible;
            }
            else if (render && browserReady)
            {
                BrowserStateOverlay.Visibility = Visibility.Collapsed;
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

        private bool ShouldActivateYoutubeRuntime()
        {
            if (HudPreferences.YoutubePanelClosed) return false;
            if (YoutubePreferences.Visible) return true;
            return widget != null && widget.GameBarDisplayMode != XboxGameBarDisplayMode.PinnedOnly;
        }

        private void ApplyYoutubeRuntimeVisibility(bool pinnedOnly)
        {
            var renderVideo = !HudPreferences.YoutubePanelClosed && (!pinnedOnly || YoutubePreferences.Visible);
            if (youtubeWebView2 != null)
                youtubeWebView2.Visibility = renderVideo && !youtubeUsingFallback ? Visibility.Visible : Visibility.Collapsed;
            if (YoutubeFallbackWebView != null)
                YoutubeFallbackWebView.Visibility = renderVideo && youtubeUsingFallback ? Visibility.Visible : Visibility.Collapsed;
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
            ClearOverlayToasts();
            voiceChatSignature = "";
            voiceChatFirstSeenAtMs.Clear();
            voiceChatExpiresAtMs.Clear();
            voiceChatRowsByKey.Clear();
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
