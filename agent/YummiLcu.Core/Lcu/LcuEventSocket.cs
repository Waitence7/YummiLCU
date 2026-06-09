using System.Net.WebSockets;
using System.Text;
using System.Text.Json;

namespace YummiLcu.Core.Lcu;

public enum LcuApiEventKind
{
    Unknown,
    Lobby,
    Gameflow,
    Matchmaking,
}

public readonly record struct LcuApiEvent(LcuApiEventKind Kind, string Uri, string EventType, string? Data);

/// <summary>LCU lockfile WebSocket — OnJsonApiEvent 구독.</summary>
public sealed class LcuEventSocket : IAsyncDisposable
{
    private static readonly HashSet<string> LobbyUris = new(StringComparer.Ordinal)
    {
        "/lol-lobby/v2/lobby",
    };

    private static readonly HashSet<string> GameflowUris = new(StringComparer.Ordinal)
    {
        "/lol-gameflow/v1/gameflow-phase",
        "/lol-gameflow/v1/session",
    };

    private static readonly HashSet<string> MatchmakingUris = new(StringComparer.Ordinal)
    {
        "/lol-matchmaking/v1/search",
        "/lol-lobby/v2/lobby/matchmaking/search-state",
    };

    private readonly int _port;
    private readonly string _password;
    private ClientWebSocket? _ws;

    public event Func<LcuApiEvent, CancellationToken, Task>? ApiEvent;

    public LcuEventSocket(int port, string password)
    {
        _port = port;
        _password = password;
    }

    public async Task RunAsync(CancellationToken ct)
    {
        _ws = new ClientWebSocket();
        _ws.Options.RemoteCertificateValidationCallback = (_, _, _, _) => true;
        var token = Convert.ToBase64String(Encoding.UTF8.GetBytes($"riot:{_password}"));
        _ws.Options.SetRequestHeader("Authorization", $"Basic {token}");

        await _ws.ConnectAsync(new Uri($"wss://127.0.0.1:{_port}"), ct);

        var subscribe = Encoding.UTF8.GetBytes("""[5,"OnJsonApiEvent"]""");
        await _ws.SendAsync(subscribe, WebSocketMessageType.Text, true, ct);

        var buf = new byte[65536];
        var pending = new StringBuilder();
        while (_ws.State == WebSocketState.Open && !ct.IsCancellationRequested)
        {
            var result = await _ws.ReceiveAsync(buf, ct);
            if (result.MessageType == WebSocketMessageType.Close)
                break;

            pending.Append(Encoding.UTF8.GetString(buf, 0, result.Count));
            if (!result.EndOfMessage)
                continue;

            var text = pending.ToString();
            pending.Clear();
            if (TryParseEvent(text, out var ev) && ApiEvent is not null)
                await ApiEvent.Invoke(ev, ct);
        }
    }

    internal static bool TryParseEvent(string text, out LcuApiEvent ev)
    {
        ev = default;
        if (string.IsNullOrWhiteSpace(text) || text[0] != '[')
            return false;
        try
        {
            using var doc = JsonDocument.Parse(text);
            var root = doc.RootElement;
            if (root.ValueKind != JsonValueKind.Array || root.GetArrayLength() < 3)
                return false;
            if (root[0].GetInt32() != 8)
                return false;
            if (root[1].GetString() != "OnJsonApiEvent")
                return false;
            var payload = root[2];
            if (payload.ValueKind != JsonValueKind.Object)
                return false;
            if (!payload.TryGetProperty("uri", out var uriEl))
                return false;
            var uri = uriEl.GetString() ?? "";
            if (!payload.TryGetProperty("eventType", out var typeEl))
                return false;
            var eventType = typeEl.GetString() ?? "";
            if (eventType is not ("Create" or "Update" or "Delete"))
                return false;

            string? data = null;
            if (payload.TryGetProperty("data", out var dataEl))
            {
                data = dataEl.ValueKind switch
                {
                    JsonValueKind.String => dataEl.GetString(),
                    JsonValueKind.Null or JsonValueKind.Undefined => null,
                    _ => dataEl.GetRawText(),
                };
            }

            var kind = ClassifyUri(uri);
            if (kind == LcuApiEventKind.Unknown)
                return false;

            ev = new LcuApiEvent(kind, uri, eventType, data);
            return true;
        }
        catch
        {
            return false;
        }
    }

    private static LcuApiEventKind ClassifyUri(string uri)
    {
        if (LobbyUris.Contains(uri)) return LcuApiEventKind.Lobby;
        if (GameflowUris.Contains(uri)) return LcuApiEventKind.Gameflow;
        if (MatchmakingUris.Contains(uri)) return LcuApiEventKind.Matchmaking;
        return LcuApiEventKind.Unknown;
    }

    public async ValueTask DisposeAsync()
    {
        if (_ws?.State == WebSocketState.Open)
        {
            try
            {
                await _ws.CloseAsync(WebSocketCloseStatus.NormalClosure, "bye", CancellationToken.None);
            }
            catch
            {
                // ignore
            }
        }
        _ws?.Dispose();
    }
}
