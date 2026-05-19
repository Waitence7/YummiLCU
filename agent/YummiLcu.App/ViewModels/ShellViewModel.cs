using System.Collections.ObjectModel;
using System.Windows;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using YummiLcu.App.Services;
using YummiLcu.Core;
using YummiLcu.Core.Lcu;
using YummiLcu.Core.Relay;

namespace YummiLcu.App.ViewModels;

public partial class ShellViewModel : ObservableObject, IDisposable
{
    private readonly AgentConfig _config;
    private readonly ILcuConnector _lcu;
    private readonly LeagueProcessMonitor _leagueMonitor = new();
    private RelaySession? _relay;
    private CancellationTokenSource? _relayCts;

    public HomeViewModel Home { get; }
    public LobbyViewModel Lobby { get; }
    public ChampSelectViewModel ChampSelect { get; }

    [ObservableProperty] private object? _currentViewModel;
    [ObservableProperty] private string _statusText = "LCU 연결 대기";
    [ObservableProperty] private string _leagueProcessHint = "롤 클라이언트 확인 중…";
    [ObservableProperty] private bool _isLcuConnected;
    [ObservableProperty] private bool _isRelayRunning;
    [ObservableProperty] private bool _testMode;
    [ObservableProperty] private string _logText = "";

    public ObservableCollection<string> LogLines { get; } = new();

    public ShellViewModel(AgentConfig config, ILcuConnector lcu)
    {
        _config = config;
        _lcu = lcu;
        _testMode = config.UiTestMode;

        Home = new HomeViewModel(lcu, config);
        Lobby = new LobbyViewModel(lcu);
        ChampSelect = new ChampSelectViewModel(lcu);

        _lcu.Log += AppendLog;
        _lcu.ConnectionChanged += connected =>
        {
            IsLcuConnected = connected;
            StatusText = connected
                ? (TestMode ? "테스트 모드" : "LCU 연결됨")
                : "LCU 미연결";
        };

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
        _ = Home.RefreshCommand.ExecuteAsync(null);
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

    private async Task NavigateAsync(object vm)
    {
        if (CurrentViewModel is ChampSelectViewModel old)
            old.StopPollingCommand.Execute(null);

        if (Application.Current.MainWindow is MainWindow mw)
        {
            await NavigationService.FadeSwapAsync(mw.PageHost, () =>
            {
                CurrentViewModel = vm;
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
        }
    }

    [RelayCommand]
    private void SetCatTheme() => ThemeService.Apply(AppTheme.Cat);

    [RelayCommand]
    private void SetCyberTheme() => ThemeService.Apply(AppTheme.Cyber);

    [RelayCommand]
    private void SetClassicTheme() => ThemeService.Apply(AppTheme.Classic);

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
        _leagueMonitor.Dispose();
        _lcu.Disconnect();
        if (_lcu is IDisposable d) d.Dispose();
    }
}
