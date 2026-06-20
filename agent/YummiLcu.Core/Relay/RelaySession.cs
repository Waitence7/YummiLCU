using System.Diagnostics;
using System.Net.WebSockets;
using System.Reflection;
using System.Text;
using System.Text.Json;
using YummiLcu.Core.Lcu;
using YummiLcu.Core.Lcu.Models;

namespace YummiLcu.Core.Relay;

public sealed class RelaySession : IAsyncDisposable
{
    private readonly AgentConfig _config;
    private readonly string _sessionId;
    private readonly string _wsToken;
    private readonly HttpClient _http = new();
    private readonly SemaphoreSlim _wsSendLock = new(1, 1);
    private readonly SemaphoreSlim _commandLock = new(1, 1);

    private ClientWebSocket? _ws;
    private CancellationTokenSource? _cts;
    private CancellationTokenSource? _sessionCts;
    private LcuClient? _lcu;
    private CancellationTokenSource? _lcuEventCts;
    private string? _lastGameflowPhase;
    private bool _eogSnapshotSent;
    private bool _wasSearching;
    private LobbyInfo _lastLobby;
    private string _idleStatus = "대기 중 (명령 수신)";
    private DateTime _lastPartyPushUtc = DateTime.MinValue;
    private DateTime _lastGameflowPushUtc = DateTime.MinValue;
    private DateTime _lastParticipantStatusPushUtc = DateTime.MinValue;
    private string? _lastParticipantStatusKey;
    private string? _lockfileSignature;
    private volatile bool _lcuEventWsOpen;
    private volatile bool _relayWsOpen;
    private bool _lcuConnected;
    private bool _lastReadyCheckActive;
    private string? _lastChampSelectFingerprint;
    private long? _discordId;

    private static readonly TimeSpan PushDebounce = TimeSpan.FromMilliseconds(300);
    private static readonly TimeSpan FallbackPollInterval = TimeSpan.FromSeconds(30);
    private static readonly string AgentVersion =
        Assembly.GetExecutingAssembly().GetName().Version?.ToString(3) ?? "0.0.0";

    public event Action<string>? StatusChanged;
    public event Action<MatchmakingStatus>? MatchmakingStatusChanged;
    public event Action<LobbyInfo>? LobbyChanged;
    public event Action<string>? Log;
    public event Action<bool>? RelayConnectionChanged;
    public event Action<bool>? LcuConnectionChanged;
    public event Action<long?>? DiscordIdChanged;
    public event Action? OAuthLinkCodeRequired;

    private TaskCompletionSource<bool>? _linkCodeTcs;

    public RelaySession(AgentConfig config, string sessionId, string wsToken)
    {
        _config = config;
        _sessionId = sessionId;
        _wsToken = wsToken;
    }

    public AgentConfig Config => _config;
    public bool IsLcuReady => _lcu is not null;
    public bool IsRelayConnected => _relayWsOpen;
    public long? DiscordId => _discordId;

    public async Task RunAsync(CancellationToken outerCt)
    {
        _cts = CancellationTokenSource.CreateLinkedTokenSource(outerCt);
        var ct = _cts.Token;
        var backoff = TimeSpan.FromSeconds(3);

        while (!ct.IsCancellationRequested)
        {
            try
            {
                var reason = await RunConnectedSessionAsync(ct);
                if (ct.IsCancellationRequested || reason == SessionEndReason.AuthExpired)
                    break;
                if (reason == SessionEndReason.UserStopped)
                    break;

                LogLine($"Relay 세션 종료 ({reason}) — {backoff.TotalSeconds:0}s 후 재연결");
                SetRelayConnected(false);
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                LogLine($"Relay 오류: {ex.Message}");
                SetRelayConnected(false);
            }

            try
            {
                await Task.Delay(backoff, ct);
            }
            catch (OperationCanceledException)
            {
                break;
            }

            backoff = TimeSpan.FromMilliseconds(Math.Min(backoff.TotalMilliseconds * 1.5, 30_000));
        }
    }

    private enum SessionEndReason
    {
        UserStopped,
        AuthExpired,
        RelayDisconnected,
    }

    private async Task<SessionEndReason> RunConnectedSessionAsync(CancellationToken ct)
    {
        _sessionCts?.Cancel();
        _sessionCts?.Dispose();
        _sessionCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        var sessionCt = _sessionCts.Token;

        LogLine($"Relay 연결 시도: {_config.RelayPublicBaseUrl}");
        _ws?.Dispose();
        _ws = new ClientWebSocket();

        try
        {
            await _ws.ConnectAsync(new Uri(_config.WsUrl(_sessionId)), sessionCt);
            var authJson = JsonSerializer.Serialize(new { type = "auth", ws_token = _wsToken });
            var authBytes = Encoding.UTF8.GetBytes(authJson);
            await _ws.SendAsync(authBytes, WebSocketMessageType.Text, true, sessionCt);
        }
        catch (Exception ex)
        {
            LogLine($"Relay WebSocket 실패: {ex.Message}");
            SetStatus("Relay 연결 실패");
            return SessionEndReason.RelayDisconnected;
        }

        SetRelayConnected(true);
        LogLine("Relay WebSocket 연결됨");

        var receiveEnded = new TaskCompletionSource<SessionEndReason>();
        _ = Task.Run(async () =>
        {
            try
            {
                await ReceiveLoopAsync(sessionCt);
                receiveEnded.TrySetResult(SessionEndReason.RelayDisconnected);
            }
            catch (OperationCanceledException)
            {
                receiveEnded.TrySetResult(SessionEndReason.UserStopped);
            }
        }, sessionCt);

        if (await PollAuthAsync(sessionCt) == AuthPollStatus.Ok)
        {
            LogLine("저장된 Discord 로그인 세션 유효 — 브라우저 생략");
        }
        else
        {
            SetStatus("브라우저 로그인 중...");
            try
            {
                Process.Start(new ProcessStartInfo(_config.LoginUrl(_sessionId)) { UseShellExecute = true });
            }
            catch (Exception ex)
            {
                LogLine($"브라우저 열기 실패: {ex.Message}");
            }

            while (!sessionCt.IsCancellationRequested)
            {
                try
                {
                    var status = await PollAuthAsync(sessionCt);
                    if (status == AuthPollStatus.Ok) break;
                    if (status == AuthPollStatus.LinkPending)
                    {
                        SetStatus("브라우저 6자리 코드 입력…");
                        LogLine("Discord 로그인 완료 — 브라우저에 표시된 6자리 코드를 입력하세요.");
                        OAuthLinkCodeRequired?.Invoke();
                        break;
                    }
                    if (status == AuthPollStatus.Expired)
                    {
                        AgentSessionStore.Clear();
                        LogLine("로그인 세션 만료 — 다시 Discord 로그인이 필요합니다.");
                        SetStatus("로그인 만료");
                        return SessionEndReason.AuthExpired;
                    }
                }
                catch (Exception ex)
                {
                    LogLine($"인증 확인 실패: {ex.Message}");
                }
                await Task.Delay(_config.AuthPollIntervalMs, sessionCt);
            }

            while (!sessionCt.IsCancellationRequested)
            {
                var status = await PollAuthAsync(sessionCt);
                if (status == AuthPollStatus.Ok) break;
                if (status == AuthPollStatus.Expired)
                {
                    AgentSessionStore.Clear();
                    LogLine("로그인 세션 만료 — 다시 Discord 로그인이 필요합니다.");
                    SetStatus("로그인 만료");
                    return SessionEndReason.AuthExpired;
                }
                await Task.Delay(_config.AuthPollIntervalMs, sessionCt);
            }
        }

        AgentSessionStore.Save(_sessionId, _wsToken, _config.RelayPublicBaseUrl);
        _ = Task.Run(() => SessionKeepAliveLoopAsync(sessionCt), sessionCt);

        SetStatus("로그인 완료 — LCU 확인 중...");
        await EnsureLcuAsync(sessionCt);

        if (_config.ApplyDefaultStatusOnConnect && _lcu is not null)
        {
            var resetResult = await AllowedActions.ExecuteAsync(
                "reset_status", new ActionContext(_lcu, _config, null));
            LogLine(resetResult.Ok ? resetResult.Message : $"기본 상메 실패: {resetResult.Message}");
        }

        _ = Task.Run(() => LcuLockfileWatchLoopAsync(sessionCt), sessionCt);
        _ = Task.Run(() => LcuWatchFallbackLoopAsync(sessionCt), sessionCt);

        _idleStatus = "대기 중 (명령 수신)";
        SetStatus(_idleStatus);
        await SendAgentHelloAsync(sessionCt);

        return await receiveEnded.Task;
    }

    public async Task<ActionResult> RunLocalCommandAsync(
        string action, JsonElement? payload = null, CancellationToken ct = default)
    {
        if (!AllowedActions.IsAllowed(action))
            return new ActionResult(false, "unknown action");
        if (action == "launch_client")
        {
            var launch = LeagueLauncher.TryLaunch();
            return new ActionResult(launch.Ok, launch.Message);
        }
        if (action == "ping")
            return await AllowedActions.ExecuteAsync(action, new ActionContext(_lcu!, _config, payload));

        if (_lcu is null)
        {
            if (action is "play_ranked_solo" or "play_normal_draft")
            {
                var (launched, launchMsg) = LeagueLauncher.TryLaunch();
                LogLine(launchMsg);
                if (!launched) return new ActionResult(false, launchMsg);
            }
            if (!await TryWaitForLcuAsync(TimeSpan.FromMinutes(4), ct))
                return new ActionResult(false, "LCU 연결 대기 시간 초과");
        }

        if (_lcu is null)
            return new ActionResult(false, "LCU 미연결");

        return await AllowedActions.ExecuteAsync(action, new ActionContext(_lcu, _config, payload));
    }

    private async Task<bool> TryWaitForLcuAsync(TimeSpan timeout, CancellationToken ct)
    {
        if (_lcu is not null) return true;
        var deadline = DateTime.UtcNow + timeout;
        while (DateTime.UtcNow < deadline && !ct.IsCancellationRequested)
        {
            var path = _config.ResolveLockfilePath() ?? LcuClient.FindLockfilePath();
            if (path is not null)
            {
                await TryConnectLcuAsync(path, ct);
                if (_lcu is not null) return true;
            }
            await Task.Delay(2500, ct);
        }
        return _lcu is not null;
    }

    private async Task EnsureLcuAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            var path = _config.ResolveLockfilePath() ?? LcuClient.FindLockfilePath();
            if (path is not null)
            {
                await TryConnectLcuAsync(path, ct);
                if (_lcu is not null) return;
            }
            else
            {
                LogLine("lockfile 대기 중...");
            }
            await Task.Delay(3000, ct);
        }
    }

    private async Task LcuLockfileWatchLoopAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            var path = _config.ResolveLockfilePath() ?? LcuClient.FindLockfilePath();
            var sig = LcuClient.ReadLockfileSignature(path);
            if (sig != _lockfileSignature)
            {
                _lockfileSignature = sig;
                if (sig is null)
                {
                    await DisconnectLcuAsync();
                }
                else if (path is not null)
                {
                    await TryConnectLcuAsync(path, ct, force: true);
                }
            }
            await Task.Delay(2000, ct);
        }
    }

    private async Task TryConnectLcuAsync(string lockfilePath, CancellationToken ct, bool force = false)
    {
        var sig = LcuClient.ReadLockfileSignature(lockfilePath);
        if (!force && sig is not null && sig == _lockfileSignature && _lcu is not null)
            return;

        var (client, error) = LcuClient.TryFromLockfile(lockfilePath);
        if (client is null)
        {
            LogLine($"lockfile: {error}");
            return;
        }

        if (_lcu is not null && sig == _lockfileSignature)
            return;

        await DisconnectLcuAsync();
        _lcu = client;
        _lockfileSignature = sig;
        SetLcuConnected(true);
        LogLine($"LCU 연결: {lockfilePath}");

        _lcuEventCts?.Cancel();
        _lcuEventCts?.Dispose();
        _lcuEventCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        _ = Task.Run(() => LcuEventLoopAsync(_lcuEventCts.Token), ct);

        await RefreshLobbyFromLcuAsync(ct);
        await RefreshMatchmakingFromLcuAsync(ct);
        var phase = await _lcu.GetGameflowPhaseAsync();
        if (phase is not null)
            await HandleGameflowPhaseAsync(phase, ct);
        await SendAgentHelloAsync(ct);
    }

    private async Task DisconnectLcuAsync()
    {
        _lcuEventCts?.Cancel();
        _lcuEventCts?.Dispose();
        _lcuEventCts = null;
        _lcuEventWsOpen = false;
        _lcu?.Dispose();
        _lcu = null;
        if (_lastLobby.IsInLobby)
        {
            _lastLobby = LobbyInfo.None;
            LobbyChanged?.Invoke(_lastLobby);
        }
        SetLcuConnected(false);
        _lastParticipantStatusKey = null;
        var ct = _sessionCts?.Token ?? _cts?.Token ?? CancellationToken.None;
        await PushParticipantStatusAsync(ct, force: true);
    }

    private async Task LcuEventLoopAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested && _lcu is not null)
        {
            try
            {
                await using var ev = new LcuEventSocket(_lcu.Port, _lcu.Password);
                ev.ApiEvent += OnLcuApiEventAsync;
                _lcuEventWsOpen = true;
                await ev.RunAsync(ct);
            }
            catch (OperationCanceledException)
            {
                break;
            }
            catch (Exception ex)
            {
                LogLine($"LCU 이벤트 WS 끊김: {ex.Message}");
            }
            finally
            {
                _lcuEventWsOpen = false;
            }

            if (!ct.IsCancellationRequested)
                await Task.Delay(3000, ct);
        }
    }

    private async Task LcuWatchFallbackLoopAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            await Task.Delay(FallbackPollInterval, ct);
            if (_lcu is null) continue;

            if (!_lcuEventWsOpen)
            {
                await RefreshLobbyFromLcuAsync(ct);
                await RefreshMatchmakingFromLcuAsync(ct);
            }

            var phase = await _lcu.GetGameflowPhaseAsync();
            if (phase is not null)
                await HandleGameflowPhaseAsync(phase, ct);
        }
    }

    private async Task OnLcuApiEventAsync(LcuApiEvent ev, CancellationToken ct)
    {
        switch (ev.Kind)
        {
            case LcuApiEventKind.Lobby:
                await RefreshLobbyFromLcuAsync(ct);
                await PushPartyLobbyUpdateAsync(ct);
                break;
            case LcuApiEventKind.Matchmaking:
                await RefreshMatchmakingFromLcuAsync(ct);
                await TryPushReadyCheckUpdateAsync(ct);
                break;
            case LcuApiEventKind.ReadyCheck:
                await TryPushReadyCheckUpdateAsync(ct);
                break;
            case LcuApiEventKind.ChampSelect:
                await TryPushChampSelectUpdateAsync(ct);
                break;
            case LcuApiEventKind.Gameflow:
            {
                var phase = ev.Data?.Trim('"');
                if (string.IsNullOrWhiteSpace(phase))
                    phase = await _lcu!.GetGameflowPhaseAsync();
                if (phase is not null)
                    await HandleGameflowPhaseAsync(phase, ct);
                break;
            }
        }
    }

    private async Task RefreshLobbyFromLcuAsync(CancellationToken ct)
    {
        if (_lcu is null) return;
        var lobby = await _lcu.GetLobbyAsync();
        if (lobby == _lastLobby) return;
        _lastLobby = lobby;
        LobbyChanged?.Invoke(lobby);
        await PushParticipantStatusAsync(ct);
    }

    private async Task RefreshMatchmakingFromLcuAsync(CancellationToken ct)
    {
        if (_lcu is null) return;
        var status = await _lcu.GetMatchmakingStatusAsync();
        MatchmakingStatusChanged?.Invoke(status);
        if (status.IsSearching)
        {
            SetStatus(status.DisplayLine);
            _wasSearching = true;
        }
        else if (_wasSearching)
        {
            SetStatus(_idleStatus);
            _wasSearching = false;
        }

    }

    private async Task HandleGameflowPhaseAsync(string phase, CancellationToken ct)
    {
        if (_lcu is null) return;

        if (_config.PreventQueueAfterDodge &&
            _lastGameflowPhase is "ChampSelect" && phase is "Lobby" or "None")
        {
            await _lcu.DeleteAsync("/lol-lobby/v2/lobby/matchmaking/search");
            LogLine("챔프선택 종료 → 매칭 중지");
        }

        if (phase is "PreEndOfGame" or "EndOfGame" or "WaitingForStats")
        {
            if (!_eogSnapshotSent && _lastGameflowPhase is "InProgress" or "PreEndOfGame")
                await TrySendGuildMatchEogSnapshotAsync(phase, ct);
        }
        else if (phase is "Lobby" or "None" or "ChampSelect" or "Matchmaking")
        {
            _eogSnapshotSent = false;
        }

        if (_lastGameflowPhase != phase)
        {
            var prev = _lastGameflowPhase;
            _lastGameflowPhase = phase;
            await PushGameflowUpdateAsync(phase, ct);
            await PushParticipantStatusAsync(ct);
            if (prev is "ChampSelect" && phase is not "ChampSelect")
                await PushChampSelectInactiveAsync(ct);
            if (phase is "ChampSelect")
                await TryPushChampSelectUpdateAsync(ct, force: true);
        }

        if (phase is "ReadyCheck" or "ChampSelect" or "Lobby" or "None" or "Matchmaking")
            await TryPushReadyCheckUpdateAsync(ct);

        if (phase is "ChampSelect")
            await TryPushChampSelectUpdateAsync(ct);
    }

    private async Task PushChampSelectInactiveAsync(CancellationToken ct)
    {
        _lastChampSelectFingerprint = null;
        await SendAgentMessageAsync(
            new { type = "champ_select_update", data = new { active = false } },
            ct);
    }

    private static string BuildChampSelectFingerprint(ChampSelectSessionInfo session)
    {
        if (!session.IsActive)
            return "inactive";
        var sb = new StringBuilder();
        sb.Append(session.Phase).Append('|').Append(session.TimerMs).Append('|');
        foreach (var a in session.Actions)
        {
            sb.Append(a.Id).Append(':')
                .Append(a.ChampionId).Append(':')
                .Append(a.Completed).Append(':')
                .Append(a.IsInProgress).Append(';');
        }
        return sb.ToString();
    }

    private static object BuildChampSelectPayload(ChampSelectSessionInfo session)
    {
        object? current = null;
        if (session.CurrentAction is { } cur)
        {
            current = new
            {
                id = cur.Id,
                type = cur.Type,
                champion_id = cur.ChampionId,
                is_mine = cur.ActorCellId < 0 || cur.ActorCellId == session.LocalPlayerCellId,
            };
        }

        return new
        {
            active = session.IsActive,
            phase = session.Phase,
            timer_ms = session.TimerMs,
            local_cell_id = session.LocalPlayerCellId,
            my_team = session.MyTeam.Select(m => new
            {
                cell_id = m.CellId,
                summoner_name = m.SummonerName,
                assigned_position = m.AssignedPosition,
                champion_id = m.ChampionId,
                champion_pick_intent = m.ChampionPickIntent,
            }),
            their_team = session.TheirTeam.Select(m => new
            {
                cell_id = m.CellId,
                summoner_name = m.SummonerName,
                assigned_position = m.AssignedPosition,
                champion_id = m.ChampionId,
                champion_pick_intent = m.ChampionPickIntent,
            }),
            actions = session.Actions.Select(a => new
            {
                id = a.Id,
                type = a.Type,
                champion_id = a.ChampionId,
                completed = a.Completed,
                is_ally_action = a.IsAllyAction,
                is_in_progress = a.IsInProgress,
                actor_cell_id = a.ActorCellId,
            }),
            current_action = current,
        };
    }

    private async Task TryPushChampSelectUpdateAsync(CancellationToken ct, bool force = false)
    {
        if (_lcu is null) return;

        var session = await _lcu.GetChampSelectSessionAsync();
        if (session is null || !session.IsActive)
        {
            if (_lastChampSelectFingerprint is not null)
                await PushChampSelectInactiveAsync(ct);
            return;
        }

        var fingerprint = BuildChampSelectFingerprint(session);
        if (!force && fingerprint == _lastChampSelectFingerprint)
            return;
        _lastChampSelectFingerprint = fingerprint;

        await SendAgentMessageAsync(
            new { type = "champ_select_update", data = BuildChampSelectPayload(session) },
            ct);
    }

    private async Task TryPushReadyCheckUpdateAsync(CancellationToken ct)
    {
        if (_lcu is null) return;

        var rc = await _lcu.GetReadyCheckAsync();
        if (rc.IsActive == _lastReadyCheckActive)
            return;

        var becameActive = rc.IsActive;
        _lastReadyCheckActive = rc.IsActive;
        await SendAgentMessageAsync(
            new
            {
                type = "ready_check_update",
                data = new
                {
                    active = rc.IsActive,
                    state = rc.State,
                    player_response = rc.PlayerResponse,
                },
            },
            ct);

        if (becameActive && _config.AutoAcceptMatch)
            _ = TryAutoAcceptMatchAsync(ct);
    }

    private async Task TryAutoAcceptMatchAsync(CancellationToken ct)
    {
        var delayMs = Random.Shared.Next(2000, 3001);
        try
        {
            await Task.Delay(delayMs, ct);
            if (_lcu is null || !_config.AutoAcceptMatch)
                return;

            var rc = await _lcu.GetReadyCheckAsync();
            if (!rc.IsActive)
                return;

            var result = await AllowedActions.ExecuteAsync(
                "accept_match",
                new ActionContext(_lcu, _config, null));
            if (result.Ok)
                LogLine($"매치 자동 수락 ({delayMs}ms 후)");
            else
                LogLine($"매치 자동 수락 실패: {result.Message}");
        }
        catch (OperationCanceledException)
        {
            // session stopped
        }
        catch (Exception ex)
        {
            LogLine($"매치 자동 수락 오류: {ex.Message}");
        }
    }

    private async Task PushPartyLobbyUpdateAsync(CancellationToken ct)
    {
        if (_lcu is null) return;
        var now = DateTime.UtcNow;
        if (now - _lastPartyPushUtc < PushDebounce) return;
        _lastPartyPushUtc = now;

        var lobby = await _lcu.GetLobbyAsync();
        var riotIds = lobby.IsInLobby
            ? await _lcu.GetLobbyMemberDisplayRiotIdsAsync()
            : Array.Empty<string>();

        await SendAgentMessageAsync(
            new
            {
                type = "party_lobby_update",
                data = new { in_lobby = lobby.IsInLobby, riot_ids_in_party = riotIds },
            },
            ct);
    }

    private async Task PushGameflowUpdateAsync(string phase, CancellationToken ct)
    {
        var now = DateTime.UtcNow;
        if (now - _lastGameflowPushUtc < PushDebounce) return;
        _lastGameflowPushUtc = now;

        await SendAgentMessageAsync(
            new { type = "gameflow_update", data = new { phase, lcu_ready = _lcu is not null } },
            ct);
    }

    private async Task PushParticipantStatusAsync(CancellationToken ct, bool force = false)
    {
        if (_ws?.State != WebSocketState.Open || _discordId is null)
            return;

        var snapshot = _lcu is not null
            ? await _lcu.BuildParticipantStatusAsync()
            : ParticipantStatusSnapshot.WaitingWithoutLcu();

        var key = $"{snapshot.Status}:{snapshot.Phase}:{snapshot.GameStartedAtMs}";
        if (!force && key == _lastParticipantStatusKey)
            return;

        _lastParticipantStatusKey = key;
        _lastParticipantStatusPushUtc = DateTime.UtcNow;

        await SendAgentMessageAsync(
            new
            {
                type = "participant_status_update",
                data = new
                {
                    status = snapshot.Status,
                    phase = snapshot.Phase,
                    game_started_at_ms = snapshot.GameStartedAtMs,
                    lcu_ready = snapshot.LcuReady,
                    agent_online = true,
                },
            },
            ct);
    }

    private async Task SendAgentHelloAsync(CancellationToken ct)
    {
        await SendAgentMessageAsync(
            new
            {
                type = "agent_hello",
                version = AgentVersion,
                lcu_ready = _lcu is not null,
                os = Environment.OSVersion.VersionString,
            },
            ct);
        await PushParticipantStatusAsync(ct, force: true);
    }

    private async Task TrySendGuildMatchEogSnapshotAsync(string phase, CancellationToken ct)
    {
        if (_lcu is null) return;

        GuildMatchLcuPayload? payload = null;
        for (var attempt = 0; attempt < 5 && payload is null; attempt++)
        {
            payload = await _lcu.BuildGuildMatchEogPayloadAsync(phase);
            if (payload is null)
                await Task.Delay(1500, ct);
        }

        if (payload is null || payload.Participants.Count < 2)
        {
            LogLine("내전 LCU 스냅샷: 참가자 정보를 아직 읽지 못했습니다.");
            return;
        }

        await SendAgentMessageAsync(new { type = "guild_match_eog", payload }, ct);
        _eogSnapshotSent = true;
        LogLine($"내전 LCU 스냅샷 전송 ({payload.Participants.Count}명)");
    }

    private async Task SendAgentMessageAsync(object message, CancellationToken ct)
    {
        if (_ws?.State != WebSocketState.Open) return;
        var json = JsonSerializer.Serialize(message);
        var bytes = Encoding.UTF8.GetBytes(json);
        await _wsSendLock.WaitAsync(ct);
        try
        {
            await _ws.SendAsync(bytes, WebSocketMessageType.Text, true, ct);
        }
        finally
        {
            _wsSendLock.Release();
        }
    }

    public async Task<bool> SubmitOAuthLinkCodeAsync(string code, CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(code)) return false;
        _linkCodeTcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        await SendAgentMessageAsync(new { type = "complete_oauth_link", code = code.Trim() }, ct);
        try
        {
            using var timeout = CancellationTokenSource.CreateLinkedTokenSource(ct);
            timeout.CancelAfter(TimeSpan.FromSeconds(20));
            await using var reg = timeout.Token.Register(() => _linkCodeTcs.TrySetResult(false));
            return await _linkCodeTcs.Task;
        }
        finally
        {
            _linkCodeTcs = null;
        }
    }

    private enum AuthPollStatus { Pending, LinkPending, Ok, Expired }

    private async Task<AuthPollStatus> PollAuthAsync(CancellationToken ct)
    {
        using var res = await _http.GetAsync(_config.AuthStatusUrl(_sessionId), ct);
        if (!res.IsSuccessStatusCode) return AuthPollStatus.Pending;
        var json = await res.Content.ReadAsStringAsync(ct);
        using var doc = JsonDocument.Parse(json);
        return doc.RootElement.GetProperty("status").GetString() switch
        {
            "ok" => AuthPollStatus.Ok,
            "link_pending" => AuthPollStatus.LinkPending,
            "expired" => AuthPollStatus.Expired,
            _ => AuthPollStatus.Pending,
        };
    }

    private async Task SessionKeepAliveLoopAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            await Task.Delay(TimeSpan.FromMinutes(3), ct);
            if (_ws?.State != WebSocketState.Open) continue;
            try
            {
                await _wsSendLock.WaitAsync(ct);
                try
                {
                    var bytes = Encoding.UTF8.GetBytes("ping");
                    await _ws.SendAsync(bytes, WebSocketMessageType.Text, true, ct);
                }
                finally
                {
                    _wsSendLock.Release();
                }
            }
            catch (OperationCanceledException) { break; }
            catch { /* ignore */ }
        }
    }

    private async Task ReceiveLoopAsync(CancellationToken ct)
    {
        if (_ws is null) return;
        var buf = new byte[8192];
        var pending = new StringBuilder();
        while (_ws.State == WebSocketState.Open && !ct.IsCancellationRequested)
        {
            var result = await _ws.ReceiveAsync(buf, ct);
            if (result.MessageType == WebSocketMessageType.Close) break;

            pending.Append(Encoding.UTF8.GetString(buf, 0, result.Count));
            if (!result.EndOfMessage) continue;

            var text = pending.ToString();
            pending.Clear();
            if (text == "ping") continue;
            await HandleMessageAsync(text);
        }
    }

    private async Task HandleMessageAsync(string text)
    {
        string? requestId = null;
        try
        {
            var ct = _sessionCts?.Token ?? _cts?.Token ?? CancellationToken.None;
            using var doc = JsonDocument.Parse(text);
            var root = doc.RootElement;
            if (root.TryGetProperty("type", out var typeEl))
            {
                var msgType = typeEl.GetString();
                if (msgType == "pong") return;
                if (msgType == "oauth_linked")
                {
                    _linkCodeTcs?.TrySetResult(true);
                    return;
                }
                if (msgType == "oauth_link_failed")
                {
                    _linkCodeTcs?.TrySetResult(false);
                    var failMsg = root.TryGetProperty("message", out var fm) ? fm.GetString() : null;
                    LogLine(string.IsNullOrWhiteSpace(failMsg) ? "링크 코드 확인 실패" : $"링크 코드 실패: {failMsg}");
                    return;
                }
                if (msgType == "session_bound")
                {
                    if (root.TryGetProperty("discord_id", out var didEl) && didEl.TryGetInt64(out var did))
                    {
                        _discordId = did;
                        DiscordIdChanged?.Invoke(did);
                        LogLine($"Discord 바인딩: {did}");
                        await SendAgentHelloAsync(ct);
                    }
                    return;
                }
                if (msgType == "request_party_snapshot")
                {
                    await PushPartyLobbyUpdateAsync(ct);
                    return;
                }
                if (msgType == "request_participant_status")
                {
                    await PushParticipantStatusAsync(ct, force: true);
                    return;
                }
                if (msgType != "command") return;
            }
            else return;

            var action = root.GetProperty("action").GetString() ?? "";
            if (root.TryGetProperty("request_id", out var reqEl))
                requestId = reqEl.GetString();

            if (!AllowedActions.IsAllowed(action))
            {
                LogLine($"거부된 action: {action}");
                await SendCommandResultAsync(requestId, false, $"unknown action: {action}");
                return;
            }

            JsonElement? payload = null;
            if (root.TryGetProperty("payload", out var p) && p.ValueKind == JsonValueKind.Object)
                payload = p.Clone();

            await _commandLock.WaitAsync(ct);
            ActionResult commandResult;
            try
            {
                commandResult = await RunLocalCommandAsync(action, payload, ct);
            }
            finally
            {
                _commandLock.Release();
            }

            LogLine($"{(commandResult.Ok ? "OK" : "FAIL")} {action}: {commandResult.Message}");
            await SendCommandResultAsync(requestId, commandResult.Ok, commandResult.Message, commandResult.Data, ct);
        }
        catch (Exception ex)
        {
            LogLine($"메시지 처리 오류: {ex.Message}");
            await SendCommandResultAsync(requestId, false, ex.Message);
        }
    }

    private async Task SendCommandResultAsync(
        string? requestId, bool ok, string message, JsonElement? data = null, CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(requestId)) return;
        if (data is null)
        {
            await SendAgentMessageAsync(
                new { type = "command_result", request_id = requestId, ok, message }, ct);
            return;
        }
        await SendAgentMessageAsync(
            new { type = "command_result", request_id = requestId, ok, message, data = data.Value }, ct);
    }

    private void SetStatus(string s) => StatusChanged?.Invoke(s);

    private void SetRelayConnected(bool connected)
    {
        if (_relayWsOpen == connected) return;
        _relayWsOpen = connected;
        RelayConnectionChanged?.Invoke(connected);
    }

    private void SetLcuConnected(bool connected)
    {
        if (_lcuConnected == connected) return;
        _lcuConnected = connected;
        LcuConnectionChanged?.Invoke(connected);
    }

    private void LogLine(string line)
    {
        AgentFileLogger.Write(line);
        Log?.Invoke(line);
    }

    public async ValueTask DisposeAsync()
    {
        _sessionCts?.Cancel();
        _cts?.Cancel();
        _lcuEventCts?.Cancel();
        if (_ws?.State == WebSocketState.Open)
        {
            try
            {
                using var closeCts = new CancellationTokenSource(TimeSpan.FromSeconds(3));
                await _ws.CloseAsync(
                    WebSocketCloseStatus.NormalClosure,
                    "bye",
                    closeCts.Token);
            }
            catch (OperationCanceledException)
            {
                // 네트워크 지연 시 강제 종료
            }
        }
        _ws?.Dispose();
        _lcu?.Dispose();
        _http.Dispose();
        _sessionCts?.Dispose();
        _cts?.Dispose();
        _lcuEventCts?.Dispose();
        _wsSendLock.Dispose();
        _commandLock.Dispose();
    }
}
