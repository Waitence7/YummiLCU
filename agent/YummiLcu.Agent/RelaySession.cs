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
    public event Action<string>? Log;

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

        _ws = new ClientWebSocket();
        await _ws.ConnectAsync(new Uri(_config.WsUrl(_sessionId)), ct);
        Log?.Invoke("WebSocket 연결됨");

        _ = Task.Run(() => ReceiveLoopAsync(ct), ct);

        while (!ct.IsCancellationRequested)
        {
            if (await PollAuthAsync(ct))
                break;
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
        SetStatus("대기 중 (명령 수신)");

        try
        {
            await Task.Delay(Timeout.Infinite, ct);
        }
        catch (OperationCanceledException)
        {
            // shutdown
        }
    }

    public async Task<(bool Ok, string Message)> RunLocalCommandAsync(string action, string? payloadText = null)
    {
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
            if (_lcu is null)
            {
                Log?.Invoke("LCU 미연결 — 명령 무시");
                return;
            }
            JsonElement? payload = root.TryGetProperty("payload", out var p) ? p.Clone() : null;
            var (ok, msg) = await AllowedActions.ExecuteAsync(action, new ActionContext(_lcu, _config, payload));
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
