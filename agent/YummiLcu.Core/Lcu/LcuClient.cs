using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using YummiLcu.Core.Lcu.Models;

namespace YummiLcu.Core.Lcu;

public sealed class LcuClient : IDisposable
{
    private readonly HttpClient _http;

    public LcuClient(int port, string password)
    {
        // Riot LCU uses self-signed certs on loopback only — reject non-local targets.
        var handler = new HttpClientHandler
        {
            ServerCertificateCustomValidationCallback = (req, _, _, _) =>
            {
                var host = req.RequestUri?.Host;
                return host is "127.0.0.1" or "localhost" or "::1";
            },
        };
        _http = new HttpClient(handler) { BaseAddress = new Uri($"https://127.0.0.1:{port}") };
        var token = Convert.ToBase64String(Encoding.UTF8.GetBytes($"riot:{password}"));
        _http.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Basic", token);
    }

    public static (LcuClient? Client, string? Error) TryFromLockfile(string lockfilePath)
    {
        if (!File.Exists(lockfilePath))
            return (null, "파일 없음");
        try
        {
            var raw = ReadLockfileText(lockfilePath);
            if (raw is null)
                return (null, "lockfile 읽기 재시도 실패 (클라이언트가 파일을 잠금)");
            if (string.IsNullOrEmpty(raw))
                return (null, "lockfile 비어 있음 (클라이언트 로딩 중일 수 있음)");
            var parts = raw.Split(':');
            if (parts.Length < 5)
                return (null, $"lockfile 형식 오류 (필드 {parts.Length}개, 5개 필요)");
            var port = int.Parse(parts[2]);
            var password = parts[3];
            return (new LcuClient(port, password), null);
        }
        catch (Exception ex)
        {
            return (null, ex.Message);
        }
    }

    private static string? ReadLockfileText(string lockfilePath)
    {
        const int maxAttempts = 8;
        for (var attempt = 0; attempt < maxAttempts; attempt++)
        {
            try
            {
                using var fs = new FileStream(lockfilePath, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
                using var reader = new StreamReader(fs, Encoding.UTF8);
                return reader.ReadToEnd().Trim();
            }
            catch (IOException) when (attempt < maxAttempts - 1)
            {
                Thread.Sleep(250);
            }
        }
        return null;
    }

    public static string? FindLockfilePath()
    {
        var overridePath = Environment.GetEnvironmentVariable("YUMMI_LCU_LOCKFILE");
        if (!string.IsNullOrWhiteSpace(overridePath) && File.Exists(overridePath))
            return overridePath;

        var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        var candidates = new List<string>();
        foreach (var drive in new[] { "C", "D", "E", "F" })
        {
            candidates.Add($@"{drive}:\Riot Games\League of Legends\lockfile");
            candidates.Add($@"{drive}:\Program Files\Riot Games\League of Legends\lockfile");
        }
        candidates.Add(Path.Combine(localAppData, "Riot Games", "Riot Client", "Config", "lockfile"));
        candidates.Add(Path.Combine(localAppData, "Riot Games", "Riot Client", "lockfile"));
        candidates.Add(Path.Combine(localAppData, "Riot Games", "League of Legends", "lockfile"));

        return candidates.FirstOrDefault(File.Exists);
    }

    private static StringContent JsonBody(string json = "{}") =>
        new(json, Encoding.UTF8, "application/json");

    public async Task<bool> PostAsync(string path) =>
        (await _http.PostAsync(path, JsonBody())).IsSuccessStatusCode;

    public async Task<bool> PostJsonAsync(string path, string json) =>
        (await _http.PostAsync(path, JsonBody(json))).IsSuccessStatusCode;

    public async Task<bool> DeleteAsync(string path) =>
        (await _http.DeleteAsync(path)).IsSuccessStatusCode;

    public async Task<bool> PutJsonAsync(string path, string json) =>
        (await _http.PutAsync(path, JsonBody(json))).IsSuccessStatusCode;

    public async Task<bool> PatchJsonAsync(string path, string json)
    {
        using var req = new HttpRequestMessage(HttpMethod.Patch, path) { Content = JsonBody(json) };
        return (await _http.SendAsync(req)).IsSuccessStatusCode;
    }

    public async Task<byte[]?> GetBytesAsync(string path)
    {
        try
        {
            var res = await _http.GetAsync(path);
            if (!res.IsSuccessStatusCode) return null;
            return await res.Content.ReadAsByteArrayAsync();
        }
        catch
        {
            return null;
        }
    }

    public async Task<JsonDocument?> GetJsonAsync(string path)
    {
        try
        {
            var res = await _http.GetAsync(path);
            if (!res.IsSuccessStatusCode) return null;
            var text = await res.Content.ReadAsStringAsync();
            if (string.IsNullOrWhiteSpace(text) || text == "null") return null;
            return JsonDocument.Parse(text);
        }
        catch
        {
            return null;
        }
    }

    public async Task<SummonerInfo?> GetCurrentSummonerAsync()
    {
        using var doc = await GetJsonAsync("/lol-summoner/v1/current-summoner");
        if (doc is null) return null;
        var r = doc.RootElement;
        return new SummonerInfo
        {
            DisplayName = r.TryGetProperty("displayName", out var n) ? n.GetString() ?? "" : "",
            SummonerId = r.TryGetProperty("summonerId", out var sid) ? sid.GetInt64() : 0,
            Level = r.TryGetProperty("summonerLevel", out var lv) ? lv.GetInt32() : 0,
            ProfileIconId = r.TryGetProperty("profileIconId", out var ic) ? ic.GetInt32() : 0,
        };
    }

    public string ProfileIconAssetPath(int iconId) =>
        $"/lol-game-data/assets/v1/profile-icons/{iconId}.jpg";

    public async Task<ChatMeInfo?> GetChatMeAsync()
    {
        using var doc = await GetJsonAsync("/lol-chat/v1/me");
        if (doc is null) return null;
        var r = doc.RootElement;
        return new ChatMeInfo
        {
            StatusMessage = r.TryGetProperty("statusMessage", out var sm) ? sm.GetString() ?? "" : "",
            Availability = r.TryGetProperty("availability", out var av) ? av.GetString() ?? "" : "",
            GameStatus = r.TryGetProperty("gameStatus", out var gs) ? gs.GetString() ?? "" : "",
        };
    }

    public async Task<IReadOnlyList<FriendInfo>> GetFriendsAsync()
    {
        using var doc = await GetJsonAsync("/lol-chat/v1/friends");
        if (doc is null || doc.RootElement.ValueKind != JsonValueKind.Array)
            return Array.Empty<FriendInfo>();

        var list = new List<FriendInfo>();
        foreach (var f in doc.RootElement.EnumerateArray())
        {
            list.Add(new FriendInfo
            {
                Puuid = f.TryGetProperty("puuid", out var p) ? p.GetString() ?? "" : "",
                GameName = f.TryGetProperty("gameName", out var gn) ? gn.GetString() ?? "" : "",
                TagLine = f.TryGetProperty("gameTag", out var gt) ? gt.GetString() ?? "" : "",
                Availability = f.TryGetProperty("availability", out var av) ? av.GetString() ?? "" : "",
            });
        }
        return list;
    }

    public async Task<ChampSelectSessionInfo?> GetChampSelectSessionAsync()
    {
        using var doc = await GetJsonAsync("/lol-champ-select/v1/session");
        if (doc is null) return null;
        var root = doc.RootElement;
        if (root.ValueKind is JsonValueKind.Null or JsonValueKind.Undefined)
            return new ChampSelectSessionInfo { IsActive = false };

        var actions = new List<ChampSelectAction>();
        if (root.TryGetProperty("actions", out var actionsArr) && actionsArr.ValueKind == JsonValueKind.Array)
        {
            foreach (var round in actionsArr.EnumerateArray())
            {
                if (round.ValueKind != JsonValueKind.Array) continue;
                foreach (var a in round.EnumerateArray())
                {
                    actions.Add(new ChampSelectAction
                    {
                        Id = a.TryGetProperty("id", out var id) ? id.GetInt32() : 0,
                        Type = a.TryGetProperty("type", out var t) ? t.GetString() ?? "" : "",
                        ChampionId = a.TryGetProperty("championId", out var cid) ? cid.GetInt32() : 0,
                        Completed = a.TryGetProperty("completed", out var c) && c.GetBoolean(),
                        IsAllyAction = a.TryGetProperty("isAllyAction", out var ally) && ally.GetBoolean(),
                        IsInProgress = a.TryGetProperty("isInProgress", out var ip) && ip.GetBoolean(),
                    });
                }
            }
        }

        return new ChampSelectSessionInfo
        {
            IsActive = true,
            Phase = root.TryGetProperty("timer", out var timer) && timer.TryGetProperty("phase", out var ph)
                ? ph.GetString() ?? "" : "",
            Actions = actions,
            LocalPlayerCellId = root.TryGetProperty("localPlayerCellId", out var cell) ? cell.GetInt32() : 0,
        };
    }

    public async Task<bool> PatchChampSelectActionAsync(int actionId, int championId, bool completed = true) =>
        await PatchJsonAsync(
            $"/lol-champ-select/v1/session/actions/{actionId}",
            $"{{\"championId\":{championId},\"completed\":{completed.ToString().ToLowerInvariant()}}}");

    public async Task<IReadOnlyList<PerkPageInfo>> GetPerkPagesAsync()
    {
        using var doc = await GetJsonAsync("/lol-perks/v1/pages");
        if (doc is null || doc.RootElement.ValueKind != JsonValueKind.Array)
            return Array.Empty<PerkPageInfo>();

        var list = new List<PerkPageInfo>();
        foreach (var p in doc.RootElement.EnumerateArray())
        {
            list.Add(new PerkPageInfo
            {
                Id = p.TryGetProperty("id", out var id) ? id.GetInt64() : 0,
                Name = p.TryGetProperty("name", out var n) ? n.GetString() ?? "" : "",
                IsActive = p.TryGetProperty("current", out var cur) && cur.GetBoolean(),
                IsDeletable = p.TryGetProperty("isDeletable", out var del) && del.GetBoolean(),
            });
        }
        return list;
    }

    public async Task<bool> SetCurrentPerkPageAsync(long pageId) =>
        await PutJsonAsync("/lol-perks/v1/pages", $"{{\"id\":{pageId}}}");

    public async Task<string?> GetGameflowPhaseAsync()
    {
        var doc = await GetJsonAsync("/lol-gameflow/v1/gameflow-phase");
        if (doc is null) return null;
        var phase = doc.RootElement.GetString();
        doc.Dispose();
        return phase;
    }

    public async Task<LobbyInfo> GetLobbyAsync()
    {
        using var doc = await GetJsonAsync("/lol-lobby/v2/lobby");
        if (doc is null) return LobbyInfo.None;

        var root = doc.RootElement;
        if (root.ValueKind is JsonValueKind.Null or JsonValueKind.Undefined)
            return LobbyInfo.None;

        var queueId = 0;
        if (root.TryGetProperty("gameConfig", out var cfg) && cfg.TryGetProperty("queueId", out var qid))
            queueId = qid.GetInt32();

        var memberCount = 0;
        if (root.TryGetProperty("members", out var members) && members.ValueKind == JsonValueKind.Array)
            memberCount = members.GetArrayLength();

        var maxMembers = 5;
        if (root.TryGetProperty("gameConfig", out var cfg2))
        {
            if (cfg2.TryGetProperty("maxTeamSize", out var mts))
                maxMembers = Math.Max(1, mts.GetInt32());
            else if (cfg2.TryGetProperty("maxLobbySize", out var mls))
                maxMembers = Math.Max(1, mls.GetInt32());
        }

        if (queueId <= 0 && memberCount <= 0) return LobbyInfo.None;
        return new LobbyInfo(true, queueId, LobbyInfo.LabelForQueue(queueId), memberCount, maxMembers);
    }

    public async Task<MatchmakingStatus> GetMatchmakingStatusAsync()
    {
        using var doc = await GetJsonAsync("/lol-matchmaking/v1/search");
        if (doc is not null)
        {
            var root = doc.RootElement;
            if (TryParseMatchmaking(root, out var status)) return status;
            if (root.TryGetProperty("isCurrentlyInQueue", out var inQ) && !inQ.GetBoolean())
                return MatchmakingStatus.Idle;
        }

        using var stateDoc = await GetJsonAsync("/lol-lobby/v2/lobby/matchmaking/search-state");
        if (stateDoc is not null && TryParseLobbySearchState(stateDoc.RootElement, out var lobbyStatus))
            return lobbyStatus;

        return MatchmakingStatus.Idle;
    }

    private static bool TryParseMatchmaking(JsonElement root, out MatchmakingStatus status)
    {
        status = MatchmakingStatus.Idle;
        var searching = root.TryGetProperty("isCurrentlyInQueue", out var inQ) && inQ.GetBoolean();
        if (root.TryGetProperty("searchState", out var stateEl))
        {
            var state = stateEl.GetString();
            if (state is "Searching" or "Found") searching = true;
            else if (state is "Idle" or "Invalid" or "Stopped") searching = false;
        }
        if (!searching) return false;

        var elapsed = root.TryGetProperty("timeInQueue", out var tq) ? MatchmakingStatus.NormalizeSeconds(tq.GetDouble()) : 0;
        var estimated = root.TryGetProperty("estimatedQueueTime", out var eq) ? MatchmakingStatus.NormalizeSeconds(eq.GetDouble()) : 0;
        status = new MatchmakingStatus(true, elapsed, estimated);
        return true;
    }

    private static bool TryParseLobbySearchState(JsonElement root, out MatchmakingStatus status)
    {
        status = MatchmakingStatus.Idle;
        if (!root.TryGetProperty("searchState", out var stateEl)) return false;
        var state = stateEl.GetString();
        if (state is not "Searching" and not "Found") return false;

        var elapsed = root.TryGetProperty("timeInQueue", out var tq) ? MatchmakingStatus.NormalizeSeconds(tq.GetDouble()) : 0;
        var estimated = root.TryGetProperty("estimatedQueueTime", out var eq) ? MatchmakingStatus.NormalizeSeconds(eq.GetDouble()) : 0;
        status = new MatchmakingStatus(true, elapsed, estimated);
        return true;
    }

    public async Task<bool> SetStatusMessageAsync(string statusMessage)
    {
        var me = await GetJsonAsync("/lol-chat/v1/me");
        if (me is null) return false;
        try
        {
            using var stream = new MemoryStream();
            using (var writer = new Utf8JsonWriter(stream))
            {
                writer.WriteStartObject();
                foreach (var prop in me.RootElement.EnumerateObject())
                {
                    if (prop.NameEquals("statusMessage")) continue;
                    prop.WriteTo(writer);
                }
                writer.WriteString("statusMessage", statusMessage);
                writer.WriteEndObject();
            }
            var json = Encoding.UTF8.GetString(stream.ToArray());
            return await PutJsonAsync("/lol-chat/v1/me", json);
        }
        finally
        {
            me.Dispose();
        }
    }

    public void Dispose() => _http.Dispose();
}
