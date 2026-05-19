using YummiLcu.Agent.Ui;

namespace YummiLcu.Agent;

public partial class MainForm : Form
{
    private readonly AgentConfig _config;
    private readonly LobbyClientPanel _lobbyPanel = new() { Dock = DockStyle.Fill, Visible = false };
    private readonly PlayBarControl _playBar = new();
    private readonly UiTestHarness _testHarness = new();
    private RelaySession? _session;
    private CancellationTokenSource? _runCts;
    private bool _inLobby;
    private bool _searching;
    private bool _testModeActive;

    private static readonly (string Label, string Action)[] ActionButtons =
    {
        ("롤 시작", "launch_client"),
        ("솔랭+매칭", "play_ranked_solo"),
        ("일반+매칭", "play_normal_draft"),
        ("매치 수락", "accept_match"),
        ("매치 거절", "decline_match"),
        ("큐 취소", "queue_cancel"),
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
        InitializeComponent();
        ApplyClientTheme();
        SetupClientChrome();
        WireTestHarness();

        _config = AgentConfig.Load();
        _testMode.Checked = _config.UiTestMode;
        _preventDodge.Checked = _config.PreventQueueAfterDodge;
        _defaultStatus.Checked = _config.ApplyDefaultStatusOnConnect;
        _statusMsg.Text = StatusMessageHelper.DefaultYummiClient;
        _lockfilePath.Text = _config.LockfilePath ?? "";

        BuildActionButtons();
        UpdateLobbyUi(LobbyInfo.None);
        _playBar.SetLobbyReady(false);
        ApplyTestMode(_config.UiTestMode, save: false);

        if (!_testModeActive)
            _status.Text = "연결 → Discord 로그인";

        _start.Click += async (_, _) => await StartAsync();
        _stop.Click += (_, _) => Stop();
        _btnSettings.Click += (_, _) => _drawer.Visible = !_drawer.Visible;
        _pickFile.Click += (_, _) => PickLockfileFile();
        _pickFolder.Click += (_, _) => PickLeagueFolder();
        _applyStatus.Click += async (_, _) => await ApplyStatusFromUiAsync();
        _testMode.CheckedChanged += (_, _) => ApplyTestMode(_testMode.Checked, save: true);
        _preventDodge.CheckedChanged += (_, _) => SaveFeatureFlags();
        _defaultStatus.CheckedChanged += (_, _) => SaveFeatureFlags();
        _btnRankedLobby.Click += async (_, _) => await RunUiActionAsync("create_ranked_lobby");
        _btnNormalLobby.Click += async (_, _) => await RunUiActionAsync("create_normal_lobby");
        _btnLeaveLobby.Click += async (_, _) => await RunUiActionAsync("leave_lobby");
        _playBar.PlayClicked += async (_, _) => await OnPlayBarClickedAsync();
    }

    private void WireTestHarness()
    {
        _testHarness.LobbyChanged += OnLobbyChanged;
        _testHarness.MatchmakingChanged += OnMatchmakingStatusChanged;
        _testHarness.Log += s => BeginInvoke(() => AppendLog(s));
    }

    private void ApplyTestMode(bool enabled, bool save)
    {
        _config.UiTestMode = enabled;
        if (save)
        {
            try { _config.Save(); } catch { /* ignore */ }
        }

        if (enabled)
        {
            if (_session is not null)
                StopRelayOnly();
            _testHarness.Start();
            _testModeActive = true;
            _start.Enabled = false;
            _stop.Enabled = false;
            _status.Text = "테스트 모드 — UI만 동작";
            _status.ForeColor = ClientTheme.Gold;
            SetLockfileInputsEnabled(false);
            AppendLog("테스트 모드 켜짐 — 연결·롤 클라이언트 없이 UI 사용 가능");
        }
        else
        {
            _testHarness.Stop();
            _testModeActive = false;
            _start.Enabled = true;
            _stop.Enabled = false;
            _status.Text = "연결 → Discord 로그인";
            _status.ForeColor = ClientTheme.TextMuted;
            SetLockfileInputsEnabled(true);
            OnLobbyChanged(LobbyInfo.None);
            OnMatchmakingStatusChanged(MatchmakingStatus.Idle);
        }
    }

    private void SetLockfileInputsEnabled(bool enabled)
    {
        _lockfilePath.Enabled = enabled;
        _pickFile.Enabled = enabled;
        _pickFolder.Enabled = enabled;
    }

    private void SetupClientChrome()
    {
        Controls.Add(_playBar);
        _playBar.BringToFront();
        _clientArea.Controls.Add(_lobbyPanel);
        _lobbyPanel.BringToFront();
        _emptyLobby.Resize += (_, _) => CenterEmptyActions();
        CenterEmptyActions();
    }

    private void ApplyClientTheme()
    {
        BackColor = ClientTheme.BgDark;
        ForeColor = ClientTheme.Text;
        _topBar.BackColor = ClientTheme.BgPanel;
        _clientArea.BackColor = ClientTheme.BgDark;
        _emptyLobby.BackColor = ClientTheme.BgDark;
        _drawer.BackColor = ClientTheme.BgPanel;
        _status.ForeColor = ClientTheme.TextMuted;
        StyleFlatButton(_start);
        StyleFlatButton(_stop);
        StyleFlatButton(_btnSettings);
        StyleFlatButton(_btnRankedLobby, accent: true);
        StyleFlatButton(_btnNormalLobby, accent: true);
        StyleFlatButton(_btnLeaveLobby);
    }

    private static void StyleFlatButton(Button btn, bool accent = false)
    {
        btn.FlatStyle = FlatStyle.Flat;
        btn.FlatAppearance.BorderColor = accent ? ClientTheme.Gold : ClientTheme.Border;
        btn.BackColor = accent ? Color.FromArgb(48, 42, 28) : ClientTheme.BgSlot;
        btn.ForeColor = accent ? ClientTheme.GoldBright : ClientTheme.Text;
    }

    private void CenterEmptyActions()
    {
        _emptyActions.Location = new Point(
            Math.Max(0, (_emptyLobby.ClientSize.Width - _emptyActions.Width) / 2),
            Math.Max(0, (_emptyLobby.ClientSize.Height - _emptyActions.Height) / 2));
    }

    private void BuildActionButtons()
    {
        foreach (var (label, action) in ActionButtons)
        {
            var btn = new Button { Text = label, AutoSize = true, Tag = action, FlatStyle = FlatStyle.Flat };
            StyleFlatButton(btn);
            btn.Click += async (_, _) => await RunUiActionAsync((string)btn.Tag!);
            _actionsFlow.Controls.Add(btn);
        }
    }

    private async Task OnPlayBarClickedAsync()
    {
        if (_searching)
        {
            await RunUiActionAsync("queue_cancel");
            return;
        }
        if (!_inLobby)
        {
            AppendLog("먼저 로비를 만드세요");
            return;
        }
        await RunUiActionAsync("queue_start");
    }

    private void UpdateLobbyUi(LobbyInfo lobby)
    {
        _inLobby = lobby.IsInLobby;
        _emptyLobby.Visible = !_inLobby;
        _lobbyPanel.Visible = _inLobby;
        _btnLeaveLobby.Visible = _inLobby;
        if (_inLobby)
            _lobbyPanel.Apply(lobby);
        if (!_searching)
            _playBar.SetLobbyReady(_inLobby);
    }

    private void SaveFeatureFlags()
    {
        _config.PreventQueueAfterDodge = _preventDodge.Checked;
        _config.ApplyDefaultStatusOnConnect = _defaultStatus.Checked;
        try { _config.Save(); } catch { /* ignore */ }
    }

    private async Task RunUiActionAsync(string action)
    {
        if (_testModeActive)
        {
            var (ok, msg) = await _testHarness.RunActionAsync(action);
            AppendLog($"{(ok ? "OK" : "FAIL")} {action}: {msg}");
            return;
        }

        if (_session is null)
        {
            AppendLog("먼저 「연결」을 눌러 주세요 (또는 테스트 모드 켜기)");
            return;
        }
        var ct = _runCts?.Token ?? CancellationToken.None;
        var (ok2, msg2) = await _session.RunLocalCommandAsync(action, ct: ct);
        AppendLog($"{(ok2 ? "OK" : "FAIL")} {action}: {msg2}");
    }

    private async Task ApplyStatusFromUiAsync()
    {
        if (_testModeActive)
        {
            var (ok, msg) = await _testHarness.RunActionAsync("set_status");
            AppendLog($"{(ok ? "OK" : "FAIL")} set_status: {msg}");
            return;
        }

        if (_session is null || !_session.IsLcuReady)
        {
            AppendLog("LCU 미연결");
            return;
        }
        var (ok2, msg2) = await _session.RunLocalCommandAsync("set_status", _statusMsg.Text);
        AppendLog($"{(ok2 ? "OK" : "FAIL")} set_status: {msg2}");
    }

    private void PickLockfileFile()
    {
        if (_testModeActive) return;
        using var dlg = new OpenFileDialog { Title = "lockfile", FileName = "lockfile", Filter = "lockfile|lockfile|*.*|*.*" };
        if (dlg.ShowDialog(this) != DialogResult.OK) return;
        _lockfilePath.Text = dlg.FileName;
        SaveLockfilePath();
    }

    private void PickLeagueFolder()
    {
        if (_testModeActive) return;
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
        if (_testModeActive) return;
        _config.LockfilePath = string.IsNullOrWhiteSpace(_lockfilePath.Text) ? null : _lockfilePath.Text.Trim();
        try { _config.Save(); } catch (Exception ex) { AppendLog($"저장 실패: {ex.Message}"); }
    }

    private Task StartAsync()
    {
        if (_testModeActive)
        {
            AppendLog("테스트 모드 — 「연결」 불필요");
            return Task.CompletedTask;
        }

        SaveLockfilePath();
        SaveFeatureFlags();
        _start.Enabled = false;
        _stop.Enabled = true;
        var sessionId = Guid.NewGuid().ToString();
        _session = new RelaySession(_config, sessionId);
        _session.StatusChanged += s => BeginInvoke(() => _status.Text = s);
        _session.LobbyChanged += OnLobbyChanged;
        _session.MatchmakingStatusChanged += OnMatchmakingStatusChanged;
        _session.Log += s => BeginInvoke(() => AppendLog(s));
        _runCts = new CancellationTokenSource();
        _ = Task.Run(async () =>
        {
            try { await _session.RunAsync(_runCts.Token); }
            catch (Exception ex) { BeginInvoke(() => AppendLog($"오류: {ex.Message}")); }
            finally { BeginInvoke(OnRelaySessionEnded); }
        });
        return Task.CompletedTask;
    }

    private void OnRelaySessionEnded()
    {
        if (_testModeActive)
            return;
        _start.Enabled = true;
        _stop.Enabled = false;
        _status.Text = "중지됨";
        OnLobbyChanged(LobbyInfo.None);
        OnMatchmakingStatusChanged(MatchmakingStatus.Idle);
    }

    private void StopRelayOnly()
    {
        _runCts?.Cancel();
        _session?.DisposeAsync().AsTask().GetAwaiter().GetResult();
        _session = null;
        _runCts = null;
    }

    private void Stop()
    {
        if (_testModeActive)
        {
            AppendLog("테스트 모드 — 「중지」 대신 설정에서 테스트 모드 해제");
            return;
        }
        StopRelayOnly();
        OnRelaySessionEnded();
    }

    private void OnLobbyChanged(LobbyInfo lobby)
    {
        if (InvokeRequired)
        {
            BeginInvoke(() => OnLobbyChanged(lobby));
            return;
        }
        UpdateLobbyUi(lobby);
    }

    private void OnMatchmakingStatusChanged(MatchmakingStatus status)
    {
        if (InvokeRequired)
        {
            BeginInvoke(() => OnMatchmakingStatusChanged(status));
            return;
        }

        _searching = status.IsSearching;
        _playBar.ApplyMatchmaking(status);
        if (!_searching)
            _playBar.SetLobbyReady(_inLobby);
    }

    private void AppendLog(string line) =>
        _log.AppendText($"[{DateTime.Now:HH:mm:ss}] {line}{Environment.NewLine}");

    protected override void OnFormClosed(FormClosedEventArgs e)
    {
        _testHarness.Dispose();
        base.OnFormClosed(e);
    }
}
