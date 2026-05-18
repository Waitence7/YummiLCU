using System.Diagnostics;
using System.Net.WebSockets;
using System.Text;
using System.Text.Json;

namespace YummiLcu.Agent;

internal sealed class RelaySession : IAsyncDisposable
{
    private readonly AgentConfig _config;
    private readonly string _sessionId;
    private readonly HttpClient _http = new();
    private ClientWebSocket? _ws;
    private CancellationTokenSource? _cts;
    private LcuClient? _lcu;
    private string? _lastGameflowPhase;

    public event Action<string>? StatusChanged;
    public event Action<MatchmakingStatus>? MatchmakingStatusChanged;
    public event Action<string>? Log;

    private bool _wasSearching;
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

        var wsUrl = _config.WsUrl(_sessionId);
        Log?.Invoke($"Relay 연결 시도: {_config.RelayPublicBaseUrl}");
        try
        {
            _ws = new ClientWebSocket();
            await _ws.ConnectAsync(new Uri(wsUrl), ct);
        }
        catch (Exception ex)
        {
            Log?.Invoke($"Relay WebSocket 실패: {ex.Message}");
            Log?.Invoke("agent.json 의 RelayPublicBaseUrl 이 https://yummi.duckdns.org 인지 확인하세요.");
            Log?.Invoke("(127.0.0.1:8790 은 이 PC에 Relay가 없으면 연결 거부됩니다)");
            SetStatus("Relay 연결 실패");
            return;
        }
        Log?.Invoke("WebSocket 연결됨");

        _ = Task.Run(() => ReceiveLoopAsync(ct), ct);

        while (!ct.IsCancellationRequested)
        {
            try
            {
                if (await PollAuthAsync(ct))
                    break;
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
        _idleStatus = "대기 중 (명령 수신)";
        SetStatus(_idleStatus);

        try
        {
            await Task.Delay(Timeout.Infinite, ct);
        }
        catch (OperationCanceledException)
        {
            // shutdown
        }
    }

    private static bool CanLaunchLeague(string action) =>
        action is "launch_client" or "play_ranked_solo" or "play_normal_draft";

    private static bool NeedsLcu(string action) =>
        action != "launch_client" && action != "ping";

    public async Task<(bool Ok, string Message)> RunLocalCommandAsync(
        string action,
        string? payloadText = null,
        CancellationToken ct = default)
    {
        if (!AllowedActions.IsAllowed(action))
            return (false, "unknown action");

        if (action == "launch_client")
            return LeagueLauncher.TryLaunch();

        if (NeedsLcu(action) && _lcu is null)
        {
            if (CanLaunchLeague(action))
            {
                var (launched, launchMsg) = LeagueLauncher.TryLaunch();
                Log?.Invoke(launchMsg);
                if (!launched)
                    return (false, launchMsg);
            }

            if (!await TryWaitForLcuAsync(TimeSpan.FromMinutes(4), ct))
                return (false, "LCU 연결 대기 시간 초과 (클라이언트 로그인 후 다시 시도)");
        }

        if (_lcu is null)
            return (false, "LCU 미연결");

        JsonDocument? payloadDoc = null;
        JsonElement? payload = null;
        if (!string.IsNullOrWhiteSpace(payloadText))
        {
            payloadDoc = JsonDocument.Parse(JsonSerializer.Serialize(new { text = payloadText }));
            payload = payloadDoc.RootElement.Clone();
        }
        try
        {
            return await AllowedActions.ExecuteAsync(action, new ActionContext(_lcu, _config, payload));
        }
        finally
        {
            payloadDoc?.Dispose();
        }
    }

    private async Task<bool> TryWaitForLcuAsync(TimeSpan timeout, CancellationToken ct)
    {
        if (_lcu is not null)
            return true;

        var deadline = DateTime.UtcNow + timeout;
        Log?.Invoke("LCU 연결 대기 중… (클라이언트 로딩·로그인)");
        while (DateTime.UtcNow < deadline && !ct.IsCancellationRequested)
        {
            var path = _config.ResolveLockfilePath();
            if (path != null && !File.Exists(path))
                path = null;
            path ??= LcuClient.FindLockfilePath();
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
                    PublishMatchmaking(MatchmakingStatus.Idle);
                    SetStatus(_idleStatus);
                }
                await Task.Delay(2000, ct);
                continue;
            }

            var status = await _lcu.GetMatchmakingStatusAsync();
            PublishMatchmaking(status);

            if (status.IsSearching)
            {
                SetStatus(status.DisplayLine);
                _wasSearching = true;
                await Task.Delay(1000, ct);
            }
            else
            {
                if (_wasSearching)
                    SetStatus(_idleStatus);
                _wasSearching = false;
                await Task.Delay(2500, ct);
            }
        }
    }

    private void PublishMatchmaking(MatchmakingStatus status) =>
        MatchmakingStatusChanged?.Invoke(status);

    private async Task GameflowWatchLoopAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested && _lcu is not null)
        {
            if (_config.PreventQueueAfterDodge)
            {
                var phase = await _lcu.GetGameflowPhaseAsync();
                if (phase is not null)
                {
                    if (_lastGameflowPhase is "ChampSelect" && phase is "Lobby" or "None")
                    {
                        await _lcu.DeleteAsync("/lol-lobby/v2/lobby/matchmaking/search");
                        Log?.Invoke("챔프선택 종료 → 매칭 중지 (즉시 재시작 방지)");
                    }
                    _lastGameflowPhase = phase;
                }
            }
            await Task.Delay(2000, ct);
        }
    }

    private async Task<bool> PollAuthAsync(CancellationToken ct)
    {
        var url = _config.AuthStatusUrl(_sessionId);
        using var res = await _http.GetAsync(url, ct);
        if (!res.IsSuccessStatusCode)
            return false;
        var json = await res.Content.ReadAsStringAsync(ct);
        using var doc = JsonDocument.Parse(json);
        var status = doc.RootElement.GetProperty("status").GetString();
        return status == "ok";
    }

    private async Task EnsureLcuAsync(CancellationToken ct)
    {
        var loggedPaths = false;
        while (!ct.IsCancellationRequested)
        {
            var path = _config.ResolveLockfilePath();
            if (path != null && !File.Exists(path))
            {
                Log?.Invoke($"agent.json LockfilePath 없음: {path}");
                path = null;
            }
            path ??= LcuClient.FindLockfilePath();
            if (path != null)
            {
                var (client, error) = LcuClient.TryFromLockfile(path);
                if (client != null)
                {
                    _lcu = client;
                    Log?.Invoke($"LCU 연결: {path}");
                    return;
                }
                Log?.Invoke($"lockfile 읽기 실패 ({path}): {error}");
            }
            else if (!loggedPaths)
            {
                loggedPaths = true;
                Log?.Invoke("lockfile 없음 — 「lockfile 파일」/「롤 폴더」로 지정하세요.");
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
        if (_ws is null)
            return;
        var buf = new byte[8192];
        while (_ws.State == WebSocketState.Open && !ct.IsCancellationRequested)
        {
            var result = await _ws.ReceiveAsync(buf, ct);
            if (result.MessageType == WebSocketMessageType.Close)
                break;
            var text = Encoding.UTF8.GetString(buf, 0, result.Count);
            await HandleMessageAsync(text);
        }
    }

    private async Task HandleMessageAsync(string text)
    {
        try
        {
            using var doc = JsonDocument.Parse(text);
            var root = doc.RootElement;
            if (root.GetProperty("type").GetString() != "command")
                return;
            var action = root.GetProperty("action").GetString() ?? "";
            if (!AllowedActions.IsAllowed(action))
            {
                Log?.Invoke($"거부된 action: {action}");
                return;
            }
            string? payloadText = null;
            if (root.TryGetProperty("payload", out var p) &&
                p.ValueKind == JsonValueKind.Object &&
                p.TryGetProperty("text", out var textEl))
                payloadText = textEl.GetString();

            var ct = _cts?.Token ?? CancellationToken.None;
            var (ok, msg) = await RunLocalCommandAsync(action, payloadText, ct);
            Log?.Invoke($"{(ok ? "OK" : "FAIL")} {action}: {msg}");
        }
        catch (Exception ex)
        {
            Log?.Invoke($"메시지 처리 오류: {ex.Message}");
        }
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
    }
}
