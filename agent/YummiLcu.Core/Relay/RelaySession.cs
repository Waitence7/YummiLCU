using System.Diagnostics;
using System.Net.WebSockets;
using System.Text;
using System.Text.Json;
using YummiLcu.Core.Lcu;
using YummiLcu.Core.Lcu.Models;

namespace YummiLcu.Core.Relay;

public sealed class RelaySession : IAsyncDisposable
{
    private readonly AgentConfig _config;
    private readonly string _sessionId;
    private readonly HttpClient _http = new();
    private ClientWebSocket? _ws;
    private CancellationTokenSource? _cts;
    private LcuClient? _lcu;
    private string? _lastGameflowPhase;
    private bool _eogSnapshotSent;
    private readonly SemaphoreSlim _wsSendLock = new(1, 1);

    public event Action<string>? StatusChanged;
    public event Action<MatchmakingStatus>? MatchmakingStatusChanged;
    public event Action<LobbyInfo>? LobbyChanged;
    public event Action<string>? Log;

    private bool _wasSearching;
    private LobbyInfo _lastLobby;
    private string _idleStatus = "대기 중 (명령 수신)";

    public RelaySession(AgentConfig config, string sessionId)
    {
        _config = config;
        _sessionId = sessionId;
    }

    public AgentConfig Config => _config;
    public bool IsLcuReady => _lcu is not null;

    public async Task RunAsync(CancellationToken outerCt)
    {
        _cts = CancellationTokenSource.CreateLinkedTokenSource(outerCt);
        var ct = _cts.Token;

        SetStatus("브라우저 로그인 중...");
        try
        {
            Process.Start(new ProcessStartInfo(_config.LoginUrl(_sessionId)) { UseShellExecute = true });
        }
        catch (Exception ex)
        {
            Log?.Invoke($"브라우저 열기 실패: {ex.Message}");
        }

        Log?.Invoke($"Relay 연결 시도: {_config.RelayPublicBaseUrl}");
        try
        {
            _ws = new ClientWebSocket();
            await _ws.ConnectAsync(new Uri(_config.WsUrl(_sessionId)), ct);
        }
        catch (Exception ex)
        {
            Log?.Invoke($"Relay WebSocket 실패: {ex.Message}");
            SetStatus("Relay 연결 실패");
            return;
        }
        Log?.Invoke("WebSocket 연결됨");

        _ = Task.Run(() => ReceiveLoopAsync(ct), ct);

        while (!ct.IsCancellationRequested)
        {
            try
            {
                if (await PollAuthAsync(ct)) break;
            }
            catch (Exception ex)
            {
                Log?.Invoke($"인증 확인 실패: {ex.Message}");
            }
            await Task.Delay(_config.AuthPollIntervalMs, ct);
        }

        SetStatus("로그인 완료 — LCU 확인 중...");
        await EnsureLcuAsync(ct);

        if (_config.ApplyDefaultStatusOnConnect && _lcu is not null)
        {
            var (ok, msg) = await AllowedActions.ExecuteAsync(
                "reset_status", new ActionContext(_lcu, _config, null));
            Log?.Invoke(ok ? msg : $"기본 상메 실패: {msg}");
        }

        _ = Task.Run(() => GameflowWatchLoopAsync(ct), ct);
        _ = Task.Run(() => MatchmakingWatchLoopAsync(ct), ct);
        _ = Task.Run(() => LobbyWatchLoopAsync(ct), ct);
        _idleStatus = "대기 중 (명령 수신)";
        SetStatus(_idleStatus);

        try { await Task.Delay(Timeout.Infinite, ct); }
        catch (OperationCanceledException) { }
    }

    public async Task<(bool Ok, string Message)> RunLocalCommandAsync(
        string action, JsonElement? payload = null, CancellationToken ct = default)
    {
        if (!AllowedActions.IsAllowed(action))
            return (false, "unknown action");
        if (action == "launch_client")
            return LeagueLauncher.TryLaunch();

        if (action != "ping" && action != "launch_client" && _lcu is null)
        {
            if (action is "play_ranked_solo" or "play_normal_draft")
            {
                var (launched, launchMsg) = LeagueLauncher.TryLaunch();
                Log?.Invoke(launchMsg);
                if (!launched) return (false, launchMsg);
            }
            if (!await TryWaitForLcuAsync(TimeSpan.FromMinutes(4), ct))
                return (false, "LCU 연결 대기 시간 초과");
        }

        if (_lcu is null)
            return (false, "LCU 미연결");

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
                var (client, error) = LcuClient.TryFromLockfile(path);
                if (client is not null)
                {
                    _lcu = client;
                    Log?.Invoke($"LCU 연결: {path}");
                    return true;
                }
                Log?.Invoke($"lockfile: {error}");
            }
            await Task.Delay(2500, ct);
        }
        return _lcu is not null;
    }

    private async Task MatchmakingWatchLoopAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            if (_lcu is null)
            {
                if (_wasSearching)
                {
                    _wasSearching = false;
                    MatchmakingStatusChanged?.Invoke(MatchmakingStatus.Idle);
                    SetStatus(_idleStatus);
                }
                await Task.Delay(2000, ct);
                continue;
            }

            var status = await _lcu.GetMatchmakingStatusAsync();
            MatchmakingStatusChanged?.Invoke(status);
            if (status.IsSearching)
            {
                SetStatus(status.DisplayLine);
                _wasSearching = true;
                await Task.Delay(1000, ct);
            }
            else
            {
                if (_wasSearching) SetStatus(_idleStatus);
                _wasSearching = false;
                await Task.Delay(2500, ct);
            }
        }
    }

    private async Task LobbyWatchLoopAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            if (_lcu is null)
            {
                if (_lastLobby.IsInLobby)
                {
                    _lastLobby = LobbyInfo.None;
                    LobbyChanged?.Invoke(_lastLobby);
                }
                await Task.Delay(2000, ct);
                continue;
            }

            var lobby = await _lcu.GetLobbyAsync();
            if (lobby != _lastLobby)
            {
                _lastLobby = lobby;
                LobbyChanged?.Invoke(lobby);
            }
            await Task.Delay(1500, ct);
        }
    }

    private async Task GameflowWatchLoopAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            if (_lcu is not null)
            {
                var phase = await _lcu.GetGameflowPhaseAsync();
                if (phase is not null)
                {
                    if (_config.PreventQueueAfterDodge &&
                        _lastGameflowPhase is "ChampSelect" && phase is "Lobby" or "None")
                    {
                        await _lcu.DeleteAsync("/lol-lobby/v2/lobby/matchmaking/search");
                        Log?.Invoke("챔프선택 종료 → 매칭 중지");
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

                    _lastGameflowPhase = phase;
                }
            }
            await Task.Delay(2000, ct);
        }
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
            Log?.Invoke("내전 LCU 스냅샷: 참가자 정보를 아직 읽지 못했습니다.");
            return;
        }

        await SendAgentMessageAsync(new { type = "guild_match_eog", payload }, ct);
        _eogSnapshotSent = true;
        Log?.Invoke($"내전 LCU 스냅샷 전송 ({payload.Participants.Count}명)");
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

    private async Task<bool> PollAuthAsync(CancellationToken ct)
    {
        using var res = await _http.GetAsync(_config.AuthStatusUrl(_sessionId), ct);
        if (!res.IsSuccessStatusCode) return false;
        var json = await res.Content.ReadAsStringAsync(ct);
        using var doc = JsonDocument.Parse(json);
        return doc.RootElement.GetProperty("status").GetString() == "ok";
    }

    private async Task EnsureLcuAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            var path = _config.ResolveLockfilePath() ?? LcuClient.FindLockfilePath();
            if (path is not null)
            {
                var (client, error) = LcuClient.TryFromLockfile(path);
                if (client is not null)
                {
                    _lcu = client;
                    Log?.Invoke($"LCU 연결: {path}");
                    return;
                }
                Log?.Invoke($"lockfile: {error}");
            }
            else
            {
                Log?.Invoke("lockfile 대기 중...");
            }
            await Task.Delay(3000, ct);
        }
    }

    private async Task ReceiveLoopAsync(CancellationToken ct)
    {
        if (_ws is null) return;
        var buf = new byte[8192];
        while (_ws.State == WebSocketState.Open && !ct.IsCancellationRequested)
        {
            var result = await _ws.ReceiveAsync(buf, ct);
            if (result.MessageType == WebSocketMessageType.Close) break;
            await HandleMessageAsync(Encoding.UTF8.GetString(buf, 0, result.Count));
        }
    }

    private async Task HandleMessageAsync(string text)
    {
        string? requestId = null;
        try
        {
            using var doc = JsonDocument.Parse(text);
            var root = doc.RootElement;
            if (root.GetProperty("type").GetString() != "command") return;
            var action = root.GetProperty("action").GetString() ?? "";
            if (root.TryGetProperty("request_id", out var reqEl))
                requestId = reqEl.GetString();

            if (!AllowedActions.IsAllowed(action))
            {
                Log?.Invoke($"거부된 action: {action}");
                await SendCommandResultAsync(requestId, false, $"unknown action: {action}");
                return;
            }

            JsonElement? payload = null;
            if (root.TryGetProperty("payload", out var p) && p.ValueKind == JsonValueKind.Object)
                payload = p.Clone();

            var ct = _cts?.Token ?? CancellationToken.None;
            var (ok, msg) = await RunLocalCommandAsync(action, payload, ct);
            Log?.Invoke($"{(ok ? "OK" : "FAIL")} {action}: {msg}");
            await SendCommandResultAsync(requestId, ok, msg, ct);
        }
        catch (Exception ex)
        {
            Log?.Invoke($"메시지 처리 오류: {ex.Message}");
            await SendCommandResultAsync(requestId, false, ex.Message);
        }
    }

    private async Task SendCommandResultAsync(
        string? requestId,
        bool ok,
        string message,
        CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(requestId)) return;
        await SendAgentMessageAsync(
            new { type = "command_result", request_id = requestId, ok, message },
            ct);
    }

    private void SetStatus(string s) => StatusChanged?.Invoke(s);

    public async ValueTask DisposeAsync()
    {
        _cts?.Cancel();
        if (_ws?.State == WebSocketState.Open)
            await _ws.CloseAsync(WebSocketCloseStatus.NormalClosure, "bye", CancellationToken.None);
        _ws?.Dispose();
        _lcu?.Dispose();
        _http.Dispose();
        _cts?.Dispose();
        _wsSendLock.Dispose();
    }
}
