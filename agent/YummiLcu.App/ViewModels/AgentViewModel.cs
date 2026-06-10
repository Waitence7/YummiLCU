using System.Collections.ObjectModel;
using System.Diagnostics;
using System.IO;
using System.Windows;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using YummiLcu.App;
using YummiLcu.Core;
using YummiLcu.Core.Relay;

namespace YummiLcu.App.ViewModels;

public partial class AgentViewModel : ObservableObject, IDisposable
{
    private readonly AgentConfig _config;
    private RelaySession? _relay;
    private CancellationTokenSource? _relayCts;
    private LeagueClientWatcher? _leagueWatcher;
    private CancellationTokenSource? _leagueWatcherCts;

    [ObservableProperty] private string _statusText = "연결 시작 → Discord 로그인";
    [ObservableProperty] private string _lockfilePath = "";
    [ObservableProperty] private bool _preventQueueAfterDodge = true;
    [ObservableProperty] private bool _applyDefaultStatusOnConnect = true;
    [ObservableProperty] private bool _autoAcceptMatch;
    [ObservableProperty] private bool _followLeagueClient = true;
    [ObservableProperty] private bool _runAtWindowsStartup;
    [ObservableProperty] private bool _isConnected;
    [ObservableProperty] private bool _relayConnected;
    [ObservableProperty] private bool _lcuConnected;
    [ObservableProperty] private string _discordIdText = "—";
    [ObservableProperty] private string _logText = "";
    [ObservableProperty] private string _oauthLinkCode = "";
    [ObservableProperty] private bool _isOAuthLinkPending;

    public ObservableCollection<string> LogLines { get; } = new();

    public AgentViewModel(AgentConfig config)
    {
        _config = config;
        _lockfilePath = config.LockfilePath ?? "";
        _preventQueueAfterDodge = config.PreventQueueAfterDodge;
        _applyDefaultStatusOnConnect = config.ApplyDefaultStatusOnConnect;
        _autoAcceptMatch = config.AutoAcceptMatch;
        _followLeagueClient = config.FollowLeagueClient;
        _runAtWindowsStartup = config.RunAtWindowsStartup || WindowsStartupHelper.IsEnabled();
    }

    public event Action? RequestApplicationShutdown;

    public void StartLeagueWatcherIfEnabled()
    {
        if (!_config.FollowLeagueClient || _leagueWatcherCts is not null)
            return;

        if (!RunAtWindowsStartup)
            AppendLog("팁: 롤 켜기 전 에이전트가 꺼져 있으면 자동 연결되지 않습니다. Windows 시작 시 자동 실행을 켜 두세요.");

        _leagueWatcher = new LeagueClientWatcher();
        _leagueWatcher.LeagueClientStarted += OnLeagueClientStarted;
        _leagueWatcher.LeagueClientStopped += OnLeagueClientStopped;
        _leagueWatcherCts = new CancellationTokenSource();
        _ = Task.Run(() => _leagueWatcher.RunAsync(_config, _leagueWatcherCts.Token));

        if (LeagueClientWatcher.IsClientPresent(_config) && !IsConnected)
            _ = StartAsync();

        if (!LeagueClientWatcher.IsClientPresent(_config))
            StatusText = "롤 클라이언트 대기 중…";
    }

    private void OnLeagueClientStarted()
    {
        System.Windows.Application.Current.Dispatcher.Invoke(() =>
        {
            if (!IsConnected)
                _ = StartAsync();
        });
    }

    private void OnLeagueClientStopped()
    {
        System.Windows.Application.Current.Dispatcher.Invoke(async () =>
        {
            if (IsConnected)
                await StopAsync();
            AppendLog("롤 클라이언트 종료 — 에이전트를 닫습니다.");
            RequestApplicationShutdown?.Invoke();
        });
    }

    public async Task CheckUpdatesOnStartupAsync()
    {
        var url = _config.UpdateManifestUrl;
        if (string.IsNullOrWhiteSpace(url) || !_config.CheckUpdatesOnStartup)
            return;

        var info = await UpdateChecker.CheckAsync(url.Trim());
        if (info is null)
            return;

        var download = info.PreferredDownloadUrl;
        var msg = $"새 버전 {info.Version} (현재 {UpdateChecker.CurrentVersion})\n{info.Notes}\n\n다운로드 페이지를 열까요?";
        var r = System.Windows.MessageBox.Show(msg, "Yummi Agent 업데이트", MessageBoxButton.YesNo, MessageBoxImage.Information);
        if (r == MessageBoxResult.Yes && !string.IsNullOrWhiteSpace(download))
        {
            try
            {
                Process.Start(new ProcessStartInfo(download) { UseShellExecute = true });
            }
            catch (Exception ex)
            {
                AppendLog($"업데이트 URL 열기 실패: {ex.Message}");
            }
        }
    }

    [RelayCommand(CanExecute = nameof(CanStart))]
    private Task StartAsync()
    {
        SaveConfig();
        IsConnected = true;
        var saved = AgentSessionStore.Load();
        var session = saved ?? AgentSessionStore.CreateNew();
        if (saved is not null)
            AppendLog("저장된 Discord 로그인 세션 사용");
        _relay = new RelaySession(_config, session.SessionId, session.WsToken);
        _relay.StatusChanged += s => StatusText = s;
        _relay.Log += AppendLog;
        _relay.RelayConnectionChanged += connected => RelayConnected = connected;
        _relay.LcuConnectionChanged += connected => LcuConnected = connected;
        _relay.DiscordIdChanged += id => DiscordIdText = id?.ToString() ?? "—";
        _relay.OAuthLinkCodeRequired += () =>
        {
            System.Windows.Application.Current.Dispatcher.Invoke(() => IsOAuthLinkPending = true);
        };
        _relayCts = new CancellationTokenSource();
        _ = Task.Run(async () =>
        {
            try
            {
                await _relay.RunAsync(_relayCts.Token);
            }
            catch (Exception ex)
            {
                System.Windows.Application.Current.Dispatcher.Invoke(() => AppendLog($"오류: {ex.Message}"));
            }
            finally
            {
                System.Windows.Application.Current.Dispatcher.Invoke(() =>
                {
                    IsConnected = false;
                    RelayConnected = false;
                    StatusText = "중지됨";
                });
            }
        });
        return Task.CompletedTask;
    }

    private bool CanStart() => !IsConnected;

    [RelayCommand(CanExecute = nameof(CanStop))]
    private async Task StopAsync()
    {
        _relayCts?.Cancel();
        if (_relay is not null)
        {
            try
            {
                await _relay.DisposeAsync().AsTask().WaitAsync(TimeSpan.FromSeconds(8));
            }
            catch (TimeoutException)
            {
                AppendLog("연결 종료 타임아웃 — 백그라운드 정리 중");
            }
        }
        if (_config.FollowLeagueClient
            && _leagueWatcher is not null
            && LeagueClientWatcher.IsClientPresent(_config))
        {
            _leagueWatcher.NotifyManualDisconnectWhileClientRunning();
        }

        _relayCts?.Dispose();
        _relayCts = null;
        _relay = null;
        IsConnected = false;
        RelayConnected = false;
        LcuConnected = false;
        IsOAuthLinkPending = false;
        OauthLinkCode = "";
        StatusText = "중지됨";
    }

    private bool CanStop() => IsConnected;

    [RelayCommand]
    private async Task SubmitOAuthLinkCodeAsync()
    {
        if (_relay is null || string.IsNullOrWhiteSpace(OauthLinkCode))
        {
            AppendLog("6자리 코드를 입력하세요.");
            return;
        }
        var ok = await _relay.SubmitOAuthLinkCodeAsync(OauthLinkCode.Trim(), _relayCts?.Token ?? CancellationToken.None);
        if (ok)
        {
            OauthLinkCode = "";
            IsOAuthLinkPending = false;
            AppendLog("Discord 연결 코드 확인됨");
        }
        else
        {
            AppendLog("코드가 올바르지 않거나 만료되었습니다.");
        }
    }

    [RelayCommand]
    private async Task ReLoginAsync()
    {
        AgentSessionStore.Clear();
        DiscordIdText = "—";
        AppendLog("Discord 세션 삭제 — 재로그인합니다.");
        if (IsConnected)
            await StopAsync();
        await StartAsync();
    }

    partial void OnIsConnectedChanged(bool value)
    {
        StartCommand.NotifyCanExecuteChanged();
        StopCommand.NotifyCanExecuteChanged();
    }

    [RelayCommand]
    private void PickLockfileFile()
    {
        var dlg = new Microsoft.Win32.OpenFileDialog
        {
            Title = "lockfile",
            FileName = "lockfile",
            Filter = "lockfile|lockfile|*.*|*.*",
        };
        if (dlg.ShowDialog() != true)
            return;
        LockfilePath = dlg.FileName;
        SaveLockfilePath();
    }

    [RelayCommand]
    private void PickLeagueFolder()
    {
        using var dlg = new System.Windows.Forms.FolderBrowserDialog
        {
            Description = "League of Legends / Riot Client 폴더",
            UseDescriptionForTitle = true,
        };
        if (dlg.ShowDialog() != System.Windows.Forms.DialogResult.OK)
            return;

        var dir = dlg.SelectedPath;
        var candidates = new[]
        {
            Path.Combine(dir, "lockfile"),
            Path.Combine(dir, "Config", "lockfile"),
            Path.Combine(dir, "League of Legends", "lockfile"),
        };
        var found = candidates.FirstOrDefault(File.Exists);
        LockfilePath = found ?? Path.Combine(dir, "lockfile");
        SaveLockfilePath();
    }

    partial void OnPreventQueueAfterDodgeChanged(bool value) => SaveFeatureFlags();
    partial void OnApplyDefaultStatusOnConnectChanged(bool value) => SaveFeatureFlags();
    partial void OnAutoAcceptMatchChanged(bool value) => SaveFeatureFlags();

    partial void OnFollowLeagueClientChanged(bool value)
    {
        _config.FollowLeagueClient = value;
        try { _config.Save(); }
        catch { /* ignore */ }

        if (value)
            StartLeagueWatcherIfEnabled();
        else
            StopLeagueWatcher();
    }

    partial void OnRunAtWindowsStartupChanged(bool value)
    {
        _config.RunAtWindowsStartup = value;
        WindowsStartupHelper.SetEnabled(value);
        try { _config.Save(); }
        catch { /* ignore */ }
    }

    private void SaveFeatureFlags()
    {
        _config.PreventQueueAfterDodge = PreventQueueAfterDodge;
        _config.ApplyDefaultStatusOnConnect = ApplyDefaultStatusOnConnect;
        _config.AutoAcceptMatch = AutoAcceptMatch;
        _config.FollowLeagueClient = FollowLeagueClient;
        try { _config.Save(); }
        catch { /* ignore */ }
    }

    private void StopLeagueWatcher()
    {
        _leagueWatcherCts?.Cancel();
        _leagueWatcherCts?.Dispose();
        _leagueWatcherCts = null;
        if (_leagueWatcher is not null)
        {
            _leagueWatcher.LeagueClientStarted -= OnLeagueClientStarted;
            _leagueWatcher.LeagueClientStopped -= OnLeagueClientStopped;
            _leagueWatcher = null;
        }
    }

    private void SaveLockfilePath()
    {
        _config.LockfilePath = string.IsNullOrWhiteSpace(LockfilePath) ? null : LockfilePath.Trim();
        try { _config.Save(); }
        catch (Exception ex) { AppendLog($"저장 실패: {ex.Message}"); }
    }

    private void SaveConfig()
    {
        SaveLockfilePath();
        SaveFeatureFlags();
        WindowsStartupHelper.SetEnabled(RunAtWindowsStartup);
        _config.RunAtWindowsStartup = RunAtWindowsStartup;
        try { _config.Save(); }
        catch { /* ignore */ }
    }

    private void AppendLog(string line)
    {
        var entry = $"[{DateTime.Now:HH:mm:ss}] {line}";
        System.Windows.Application.Current.Dispatcher.Invoke(() =>
        {
            LogLines.Add(entry);
            while (LogLines.Count > 500)
                LogLines.RemoveAt(0);
            LogText = string.Join(Environment.NewLine, LogLines);
        });
    }

    public void Dispose()
    {
        StopLeagueWatcher();
        _relayCts?.Cancel();
        _relay?.DisposeAsync().AsTask().GetAwaiter().GetResult();
    }
}
