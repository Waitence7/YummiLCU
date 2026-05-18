using System.Diagnostics;

namespace YummiLcu.Agent;

internal sealed class MainForm : Form
{
    private readonly AgentConfig _config;
    private readonly Label _status = new() { AutoSize = true, MaximumSize = new Size(520, 0) };
    private readonly TextBox _lockfilePath = new() { Width = 340, PlaceholderText = "lockfile 경로" };
    private readonly TextBox _statusMsg = new() { Width = 340, Multiline = true, Height = 48, ScrollBars = ScrollBars.Vertical };
    private readonly CheckBox _preventDodge = new() { Text = "닷지 후 매칭 자동 재시작 방지", AutoSize = true, Checked = true };
    private readonly CheckBox _defaultStatus = new() { Text = "연결 시 기본 상메 적용", AutoSize = true, Checked = true };
    private readonly TextBox _log = new()
    {
        Multiline = true,
        ReadOnly = true,
        ScrollBars = ScrollBars.Vertical,
        Dock = DockStyle.Fill,
    };
    private readonly Button _start = new() { Text = "연결 시작", AutoSize = true };
    private readonly Button _stop = new() { Text = "중지", AutoSize = true, Enabled = false };
    private RelaySession? _session;
    private CancellationTokenSource? _runCts;

    private static readonly (string Label, string Action)[] ActionButtons =
    {
        ("매치 수락", "accept_match"),
        ("매치 거절", "decline_match"),
        ("큐 시작", "queue_start"),
        ("큐 취소", "queue_cancel"),
        ("로비 나가기", "leave_lobby"),
        ("파티 준비", "party_ready"),
        ("챔프 리롤", "champ_reroll"),
        ("닷지", "dodge"),
        ("재접속", "reconnect"),
        ("보상 전부", "claim_all_rewards"),
        ("기본 상메", "reset_status"),
        ("클라 종료", "quit_client"),
    };

    public MainForm()
    {
        _config = AgentConfig.Load();
        _preventDodge.Checked = _config.PreventQueueAfterDodge;
        _defaultStatus.Checked = _config.ApplyDefaultStatusOnConnect;
        Text = "YummiLcu Agent";
        Size = new Size(600, 640);
        MinimumSize = new Size(520, 500);

        var root = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            ColumnCount = 1,
            RowCount = 5,
            Padding = new Padding(8),
        };
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));

        var rowConn = new FlowLayoutPanel { AutoSize = true, WrapContents = false };
        rowConn.Controls.Add(_start);
        rowConn.Controls.Add(_stop);
        rowConn.Controls.Add(_status);

        var rowLock = new FlowLayoutPanel { AutoSize = true, WrapContents = true };
        rowLock.Controls.Add(new Label { Text = "lockfile:", AutoSize = true, Padding = new Padding(0, 6, 0, 0) });
        rowLock.Controls.Add(_lockfilePath);
        var pickFile = new Button { Text = "파일", AutoSize = true };
        var pickFolder = new Button { Text = "롤 폴더", AutoSize = true };
        pickFile.Click += (_, _) => PickLockfileFile();
        pickFolder.Click += (_, _) => PickLeagueFolder();
        rowLock.Controls.Add(pickFile);
        rowLock.Controls.Add(pickFolder);

        var grpSettings = new GroupBox { Text = "설정 / 상메", AutoSize = true, Padding = new Padding(8) };
        var settingsPanel = new FlowLayoutPanel { AutoSize = true, FlowDirection = FlowDirection.TopDown, WrapContents = false };
        settingsPanel.Controls.Add(_preventDodge);
        settingsPanel.Controls.Add(_defaultStatus);
        var rowStatus = new FlowLayoutPanel { AutoSize = true, WrapContents = true };
        rowStatus.Controls.Add(new Label { Text = "상메:", AutoSize = true });
        rowStatus.Controls.Add(_statusMsg);
        var applyStatus = new Button { Text = "상메 적용", AutoSize = true };
        applyStatus.Click += async (_, _) => await ApplyStatusFromUiAsync();
        rowStatus.Controls.Add(applyStatus);
        settingsPanel.Controls.Add(rowStatus);
        _statusMsg.Text = StatusMessageHelper.DefaultYummiClient;
        grpSettings.Controls.Add(settingsPanel);

        var grpActions = new GroupBox { Text = "LCU 명령 (로컬)", AutoSize = true, Padding = new Padding(8) };
        var actionsFlow = new FlowLayoutPanel { AutoSize = true, WrapContents = true, MaximumSize = new Size(560, 0) };
        foreach (var (label, action) in ActionButtons)
        {
            var btn = new Button { Text = label, AutoSize = true, Tag = action };
            btn.Click += async (_, _) => await RunUiActionAsync((string)btn.Tag!);
            actionsFlow.Controls.Add(btn);
        }
        grpActions.Controls.Add(actionsFlow);

        root.Controls.Add(rowConn, 0, 0);
        root.Controls.Add(rowLock, 0, 1);
        root.Controls.Add(grpSettings, 0, 2);
        root.Controls.Add(grpActions, 0, 3);
        root.Controls.Add(_log, 0, 4);
        Controls.Add(root);

        _lockfilePath.Text = _config.LockfilePath ?? "";
        _status.Text = "연결 시작 → Discord 로그인";

        _start.Click += async (_, _) => await StartAsync();
        _stop.Click += (_, _) => Stop();
        _preventDodge.CheckedChanged += (_, _) => SaveFeatureFlags();
        _defaultStatus.CheckedChanged += (_, _) => SaveFeatureFlags();
        Shown += async (_, _) => await CheckUpdatesAsync();
    }

    private async Task CheckUpdatesAsync()
    {
        var url = _config.UpdateManifestUrl;
        if (string.IsNullOrWhiteSpace(url) || !_config.CheckUpdatesOnStartup)
            return;
        var info = await UpdateChecker.CheckAsync(url.Trim());
        if (info is null)
            return;
        var msg = $"새 버전 {info.Version} (현재 {UpdateChecker.CurrentVersion})\n{info.Notes}\n\n다운로드 페이지를 열까요?";
        var r = MessageBox.Show(msg, "Yummi Agent 업데이트", MessageBoxButtons.YesNo, MessageBoxIcon.Information);
        if (r == DialogResult.Yes && !string.IsNullOrWhiteSpace(info.Url))
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

    private void SaveFeatureFlags()
    {
        _config.PreventQueueAfterDodge = _preventDodge.Checked;
        _config.ApplyDefaultStatusOnConnect = _defaultStatus.Checked;
        try { _config.Save(); } catch { /* ignore */ }
    }

    private async Task RunUiActionAsync(string action)
    {
        if (_session is null || !_session.IsLcuReady)
        {
            AppendLog("LCU 미연결 — 먼저 연결 시작");
            return;
        }
        var (ok, msg) = await _session.RunLocalCommandAsync(action);
        AppendLog($"{(ok ? "OK" : "FAIL")} {action}: {msg}");
    }

    private async Task ApplyStatusFromUiAsync()
    {
        if (_session is null || !_session.IsLcuReady)
        {
            AppendLog("LCU 미연결");
            return;
        }
        var (ok, msg) = await _session.RunLocalCommandAsync("set_status", _statusMsg.Text);
        AppendLog($"{(ok ? "OK" : "FAIL")} set_status: {msg}");
    }

    private void PickLockfileFile()
    {
        using var dlg = new OpenFileDialog { Title = "lockfile", FileName = "lockfile", Filter = "lockfile|lockfile|*.*|*.*" };
        if (dlg.ShowDialog(this) != DialogResult.OK) return;
        _lockfilePath.Text = dlg.FileName;
        SaveLockfilePath();
    }

    private void PickLeagueFolder()
    {
        using var dlg = new FolderBrowserDialog
        {
            Description = "League of Legends / Riot Client 폴더",
            UseDescriptionForTitle = true,
        };
        if (dlg.ShowDialog(this) != DialogResult.OK) return;
        var dir = dlg.SelectedPath;
        var candidates = new[]
        {
            Path.Combine(dir, "lockfile"),
            Path.Combine(dir, "Config", "lockfile"),
            Path.Combine(dir, "League of Legends", "lockfile"),
        };
        var found = candidates.FirstOrDefault(File.Exists);
        _lockfilePath.Text = found ?? Path.Combine(dir, "lockfile");
        SaveLockfilePath();
    }

    private void SaveLockfilePath()
    {
        _config.LockfilePath = string.IsNullOrWhiteSpace(_lockfilePath.Text) ? null : _lockfilePath.Text.Trim();
        try { _config.Save(); } catch (Exception ex) { AppendLog($"저장 실패: {ex.Message}"); }
    }

    private Task StartAsync()
    {
        SaveLockfilePath();
        SaveFeatureFlags();
        _start.Enabled = false;
        _stop.Enabled = true;
        var sessionId = Guid.NewGuid().ToString();
        _session = new RelaySession(_config, sessionId);
        _session.StatusChanged += s => BeginInvoke(() => _status.Text = s);
        _session.Log += s => BeginInvoke(() => AppendLog(s));
        _runCts = new CancellationTokenSource();
        _ = Task.Run(async () =>
        {
            try { await _session.RunAsync(_runCts.Token); }
            catch (Exception ex) { BeginInvoke(() => AppendLog($"오류: {ex.Message}")); }
            finally { BeginInvoke(() => { _start.Enabled = true; _stop.Enabled = false; }); }
        });
        return Task.CompletedTask;
    }

    private void Stop()
    {
        _runCts?.Cancel();
        _session?.DisposeAsync().AsTask().GetAwaiter().GetResult();
        _status.Text = "중지됨";
        _start.Enabled = true;
        _stop.Enabled = false;
    }

    private void AppendLog(string line) =>
        _log.AppendText($"[{DateTime.Now:HH:mm:ss}] {line}{Environment.NewLine}");
}
