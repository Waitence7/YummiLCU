using System.Collections.ObjectModel;
using System.Windows;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using YummiLcu.App.Composition;
using YummiLcu.App.Infrastructure.Atmosphere;
using YummiLcu.App.Infrastructure.Events;
using YummiLcu.App.Infrastructure.Lcu;
using YummiLcu.App.Infrastructure.Pet;
using YummiLcu.App.Infrastructure.Settings;
using YummiLcu.App.Infrastructure.Toasts;
using YummiLcu.App.Services;
using YummiLcu.Core;
using YummiLcu.Core.Lcu;
using YummiLcu.Core.Relay;

namespace YummiLcu.App.ViewModels;

public partial class ShellViewModel : ObservableObject, IDisposable
{
    private readonly AgentConfig _config;
    private readonly ILcuConnector _lcu;
    private readonly AppServices _services;
    private readonly LeagueProcessMonitor _leagueMonitor = new();
    private RelaySession? _relay;
    private CancellationTokenSource? _relayCts;
    private IDisposable? _petStateSubscription;
    private IDisposable? _atmosphereStateSubscription;
    private IDisposable? _toastSubscription;

    public HomeViewModel Home { get; }
    public LobbyViewModel Lobby { get; }
    public ChampSelectViewModel ChampSelect { get; }
    public SettingsViewModel Settings { get; }

    [ObservableProperty] private object? _currentViewModel;
    [ObservableProperty] private bool _isHomeActive = true;
    [ObservableProperty] private bool _isLobbyActive;
    [ObservableProperty] private bool _isChampSelectActive;
    [ObservableProperty] private bool _isSettingsActive;
    [ObservableProperty] private string _statusText = "LCU 연결 대기";
    [ObservableProperty] private string _leagueProcessHint = "롤 클라이언트 확인 중…";
    [ObservableProperty] private bool _isLcuConnected;
    [ObservableProperty] private bool _enablePetPlaceholder = true;
    [ObservableProperty] private bool _enableAtmosphereReactions = true;
    [ObservableProperty] private AppGameState _currentGameState = AppGameState.Disconnected;
    [ObservableProperty] private AtmosphereState _currentAtmosphereState = AtmosphereState.Dimmed;
    [ObservableProperty] private PetState _currentPetState = PetState.Sleeping;
    [ObservableProperty] private bool _isRelayRunning;
    [ObservableProperty] private bool _testMode;
    [ObservableProperty] private string _logText = "";

    public ObservableCollection<string> LogLines { get; } = new();
    public ObservableCollection<ToastNotification> Toasts { get; } = new();

    public ShellViewModel(AgentConfig config, ILcuConnector lcu, AppServices services)
    {
        _config = config;
        _lcu = lcu;
        _services = services;
        _testMode = config.UiTestMode;
        _services.ShellState.SetTestMode(_testMode);

        Home = new HomeViewModel(lcu, config);
        Lobby = new LobbyViewModel(lcu);
        ChampSelect = new ChampSelectViewModel(lcu);
        Settings = new SettingsViewModel(services);

        _lcu.Log += AppendLog;
        _services.LcuStates.ConnectionChanged += OnLcuConnectionChanged;
        _services.LcuStates.GameStateChanged += OnLcuGameStateChanged;
        _services.Preferences.PreferencesChanged += OnPreferencesChanged;
        ApplyPreferences(_services.Preferences.Current);
        _atmosphereStateSubscription = _services.Events.Subscribe<AtmosphereStateChangedEvent>(OnAtmosphereStateChanged);
        CurrentAtmosphereState = _services.State.CurrentAtmosphereState;
        _petStateSubscription = _services.Events.Subscribe<PetStateChangedEvent>(OnPetStateChanged);
        CurrentPetState = _services.State.CurrentPetState;
        _toastSubscription = _services.Events.Subscribe<ToastRequestedEvent>(OnToastRequested);
        _services.LcuStates.Attach(_lcu);

        _leagueMonitor.LeagueStarted += OnLeagueStarted;
        _leagueMonitor.LeagueExited += OnLeagueExited;
        if (!_testMode)
            _leagueMonitor.Start();

        if (_testMode)
        {
            _lcu.SetTestMode(true);
            IsLcuConnected = true;
            StatusText = "테스트 모드 — 롤 없이 UI 확인";
            AppendLog("테스트 모드로 시작 (agent.json UiTestMode 또는 --test)");
        }

        RefreshLeagueHint();

        CurrentViewModel = Home;
        SetCurrentPage("Home");
        _ = Home.RefreshCommand.ExecuteAsync(null);
    }

    private void OnPreferencesChanged(InteractionPreferences preferences)
    {
        Application.Current?.Dispatcher.Invoke(() => ApplyPreferences(preferences));
    }

    private void ApplyPreferences(InteractionPreferences preferences)
    {
        EnablePetPlaceholder = preferences.EnablePetPlaceholder;
        EnableAtmosphereReactions = preferences.EnableAtmosphereReactions;
    }

    private void OnLcuConnectionChanged(bool connected)
    {
        Application.Current?.Dispatcher.Invoke(() =>
        {
            IsLcuConnected = connected;
            StatusText = connected
                ? (TestMode ? "테스트 모드" : "LCU 연결됨")
                : "LCU 미연결";
        });
    }

    private void OnLcuGameStateChanged(AppGameState state)
    {
        Application.Current?.Dispatcher.Invoke(() => CurrentGameState = state);
    }

    private void OnAtmosphereStateChanged(AtmosphereStateChangedEvent appEvent)
    {
        Application.Current?.Dispatcher.Invoke(() => CurrentAtmosphereState = appEvent.State);
    }

    private void OnPetStateChanged(PetStateChangedEvent appEvent)
    {
        Application.Current?.Dispatcher.Invoke(() => CurrentPetState = appEvent.State);
    }

    private void OnToastRequested(ToastRequestedEvent appEvent)
    {
        Application.Current?.Dispatcher.Invoke(() =>
        {
            var toast = new ToastNotification(appEvent.Type, appEvent.Title, appEvent.Message);
            Toasts.Insert(0, toast);
            while (Toasts.Count > 4)
                Toasts.RemoveAt(Toasts.Count - 1);

            _ = DismissToastAfterDelayAsync(toast);
        });
    }

    private async Task DismissToastAfterDelayAsync(ToastNotification toast)
    {
        await Task.Delay(TimeSpan.FromSeconds(4.2));
        await CloseToastAsync(toast);
    }

    [RelayCommand]
    private async Task DismissToastAsync(ToastNotification? toast)
    {
        if (toast is null) return;
        await CloseToastAsync(toast);
    }

    private async Task CloseToastAsync(ToastNotification toast)
    {
        var shouldClose = false;
        Application.Current?.Dispatcher.Invoke(() =>
        {
            if (!Toasts.Contains(toast) || toast.IsClosing) return;
            toast.IsClosing = true;
            shouldClose = true;
        });

        if (!shouldClose) return;
        await Task.Delay(TimeSpan.FromMilliseconds(180));
        Application.Current?.Dispatcher.Invoke(() => Toasts.Remove(toast));
    }

    private void OnLeagueStarted()
    {
        if (TestMode) return;
        Application.Current?.Dispatcher.Invoke(() =>
        {
            RefreshLeagueHint();
            AppendLog("롤 클라이언트 실행 감지");
            if (!_lcu.IsConnected)
                StatusText = "롤 실행됨 — LCU 연결 가능";
        });
    }

    private void OnLeagueExited()
    {
        if (TestMode) return;
        Application.Current?.Dispatcher.Invoke(async () =>
        {
            RefreshLeagueHint();
            AppendLog("롤 클라이언트 종료 감지 — LCU 연결 해제");

            _lcu.Disconnect();
            IsLcuConnected = false;
            StatusText = "롤 클라이언트 꺼짐 — LCU 미연결";

            if (IsRelayRunning)
                await StopRelayAsync();
        });
    }

    private void RefreshLeagueHint()
    {
        if (TestMode)
        {
            LeagueProcessHint = "테스트 모드 — 롤 클라이언트 불필요";
            return;
        }
        LeagueProcessHint = _leagueMonitor.IsLeagueRunning
            ? "롤 클라이언트 실행 중"
            : "롤 클라이언트 꺼짐";
    }

    partial void OnTestModeChanged(bool value)
    {
        _config.UiTestMode = value;
        try { _config.Save(); } catch { /* ignore */ }

        if (value)
            _leagueMonitor.Stop();
        else
            _leagueMonitor.Start();

        _lcu.SetTestMode(value);
        _services.ShellState.SetTestMode(value);
        IsLcuConnected = _lcu.IsConnected;
        StatusText = value
            ? "테스트 모드 — 롤 없이 UI 확인"
            : (_lcu.IsConnected ? "LCU 연결됨" : "LCU 미연결");
        RefreshLeagueHint();

        if (value)
            _ = Home.RefreshCommand.ExecuteAsync(null);
    }

    [RelayCommand]
    private async Task NavigateHomeAsync() => await NavigateAsync(Home);

    [RelayCommand]
    private async Task NavigateLobbyAsync() => await NavigateAsync(Lobby);

    [RelayCommand]
    private async Task NavigateChampSelectAsync()
    {
        await NavigateAsync(ChampSelect);
        ChampSelect.StartPollingCommand.Execute(null);
    }

    [RelayCommand]
    private async Task NavigateSettingsAsync() => await NavigateAsync(Settings);

    private async Task NavigateAsync(object vm)
    {
        if (CurrentViewModel is ChampSelectViewModel old)
            old.StopPollingCommand.Execute(null);

        var pageName = PageNameFor(vm);
        if (Application.Current.MainWindow is MainWindow mw)
        {
            await _services.Animations.FadeSwapAsync(mw.PageHost, () =>
            {
                CurrentViewModel = vm;
                SetCurrentPage(pageName);
                if (vm is LobbyViewModel lobby)
                    return lobby.LoadedCommand.ExecuteAsync(null)!;
                if (vm is HomeViewModel home)
                    return home.RefreshCommand.ExecuteAsync(null)!;
                return Task.CompletedTask;
            });
        }
        else
        {
            CurrentViewModel = vm;
            SetCurrentPage(pageName);
        }
    }

    [RelayCommand]
    private void SetCatTheme() => _services.Themes.Apply(AppTheme.Cat);

    [RelayCommand]
    private void SetCyberTheme() => _services.Themes.Apply(AppTheme.Cyber);

    [RelayCommand]
    private void SetClassicTheme() => _services.Themes.Apply(AppTheme.Classic);

    [RelayCommand]
    private async Task ConnectLcuAsync()
    {
        if (TestMode) return;
        await _lcu.ConnectAsync(_config);
    }

    [RelayCommand]
    private async Task StartRelayAsync()
    {
        if (_relay is not null) return;
        _relayCts = new CancellationTokenSource();
        var sessionId = Guid.NewGuid().ToString();
        _relay = new RelaySession(_config, sessionId);
        _relay.StatusChanged += s => StatusText = s;
        _relay.Log += AppendLog;
        IsRelayRunning = true;
        _ = Task.Run(async () =>
        {
            try { await _relay.RunAsync(_relayCts.Token); }
            catch (Exception ex) { AppendLog($"Relay: {ex.Message}"); }
            finally
            {
                IsRelayRunning = false;
                _relay = null;
            }
        });
        await Task.CompletedTask;
    }

    [RelayCommand]
    private async Task StopRelayAsync()
    {
        _relayCts?.Cancel();
        if (_relay is not null)
            await _relay.DisposeAsync();
        _relay = null;
        IsRelayRunning = false;
    }

    partial void OnIsRelayRunningChanged(bool value)
    {
        _services.ShellState.SetRelayRunning(value);
    }

    private void SetCurrentPage(string pageName)
    {
        IsHomeActive = pageName == "Home";
        IsLobbyActive = pageName == "Lobby";
        IsChampSelectActive = pageName == "ChampSelect";
        IsSettingsActive = pageName == "Settings";
        _services.ShellState.SetCurrentPage(pageName);
    }

    private static string PageNameFor(object vm) => vm switch
    {
        HomeViewModel => "Home",
        LobbyViewModel => "Lobby",
        ChampSelectViewModel => "ChampSelect",
        SettingsViewModel => "Settings",
        _ => vm.GetType().Name,
    };

    private void AppendLog(string line)
    {
        Application.Current?.Dispatcher.Invoke(() =>
        {
            LogLines.Insert(0, $"[{DateTime.Now:HH:mm:ss}] {line}");
            while (LogLines.Count > 200) LogLines.RemoveAt(LogLines.Count - 1);
            LogText = string.Join(Environment.NewLine, LogLines.Take(30).Reverse());
        });
    }

    public void Dispose()
    {
        _services.LcuStates.ConnectionChanged -= OnLcuConnectionChanged;
        _services.LcuStates.GameStateChanged -= OnLcuGameStateChanged;
        _services.LcuStates.Detach();
        _services.Preferences.PreferencesChanged -= OnPreferencesChanged;
        _atmosphereStateSubscription?.Dispose();
        _atmosphereStateSubscription = null;
        _petStateSubscription?.Dispose();
        _petStateSubscription = null;
        _toastSubscription?.Dispose();
        _toastSubscription = null;
        _leagueMonitor.Dispose();
        _lcu.Disconnect();
        if (_lcu is IDisposable d) d.Dispose();
    }
}
