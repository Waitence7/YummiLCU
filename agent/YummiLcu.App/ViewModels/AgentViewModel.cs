using System.Collections.ObjectModel;
using System.Diagnostics;
using System.IO;
using System.Security.Cryptography;
using System.Windows;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using YummiLcu.Core;
using YummiLcu.Core.Relay;

namespace YummiLcu.App.ViewModels;

public partial class AgentViewModel : ObservableObject, IDisposable
{
    private readonly AgentConfig _config;
    private RelaySession? _relay;
    private CancellationTokenSource? _relayCts;

    [ObservableProperty] private string _statusText = "연결 시작 → Discord 로그인";
    [ObservableProperty] private string _lockfilePath = "";
    [ObservableProperty] private bool _preventQueueAfterDodge = true;
    [ObservableProperty] private bool _applyDefaultStatusOnConnect = true;
    [ObservableProperty] private bool _isConnected;
    [ObservableProperty] private string _logText = "";

    public ObservableCollection<string> LogLines { get; } = new();

    public AgentViewModel(AgentConfig config)
    {
        _config = config;
        _lockfilePath = config.LockfilePath ?? "";
        _preventQueueAfterDodge = config.PreventQueueAfterDodge;
        _applyDefaultStatusOnConnect = config.ApplyDefaultStatusOnConnect;
    }

    public async Task CheckUpdatesOnStartupAsync()
    {
        var url = _config.UpdateManifestUrl;
        if (string.IsNullOrWhiteSpace(url) || !_config.CheckUpdatesOnStartup)
            return;

        var info = await UpdateChecker.CheckAsync(url.Trim());
        if (info is null)
            return;

        var msg = $"새 버전 {info.Version} (현재 {UpdateChecker.CurrentVersion})\n{info.Notes}\n\n다운로드 페이지를 열까요?";
        var r = System.Windows.MessageBox.Show(msg, "Yummi Agent 업데이트", MessageBoxButton.YesNo, MessageBoxImage.Information);
        if (r == MessageBoxResult.Yes && !string.IsNullOrWhiteSpace(info.Url))
        {
            try
            {
                Process.Start(new ProcessStartInfo(info.Url) { UseShellExecute = true });
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
        var sessionId = Guid.NewGuid().ToString();
        var wsToken = Convert.ToBase64String(RandomNumberGenerator.GetBytes(32));
        _relay = new RelaySession(_config, sessionId, wsToken);
        _relay.StatusChanged += s => StatusText = s;
        _relay.Log += AppendLog;
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
            await _relay.DisposeAsync();
        _relay = null;
        IsConnected = false;
        StatusText = "중지됨";
    }

    private bool CanStop() => IsConnected;

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

    private void SaveFeatureFlags()
    {
        _config.PreventQueueAfterDodge = PreventQueueAfterDodge;
        _config.ApplyDefaultStatusOnConnect = ApplyDefaultStatusOnConnect;
        try { _config.Save(); }
        catch { /* ignore */ }
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
        _relayCts?.Cancel();
        _relay?.DisposeAsync().AsTask().GetAwaiter().GetResult();
    }
}
