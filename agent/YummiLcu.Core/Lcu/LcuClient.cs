using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using YummiLcu.Core.Lcu.Models;

namespace YummiLcu.Core.Lcu;

public sealed class LcuClient : IDisposable
{
    private readonly HttpClient _http;

    public int Port { get; }
    public string Password { get; }

    public LcuClient(int port, string password)
    {
        Port = port;
        Password = password;
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
                        ActorCellId = a.TryGetProperty("actorCellId", out var ac) ? ac.GetInt32() : -1,
                    });
                }
            }
        }

        var localCellId = root.TryGetProperty("localPlayerCellId", out var cell) ? cell.GetInt32() : 0;
        var timerMs = 0;
        var phase = "";
        if (root.TryGetProperty("timer", out var timer))
        {
            if (timer.TryGetProperty("phase", out var ph))
                phase = ph.GetString() ?? "";
            if (timer.TryGetProperty("adjustedTimeLeftInPhase", out var left))
                timerMs = left.GetInt32();
        }

        ChampSelectAction? currentAction = null;
        foreach (var action in actions)
        {
            if (!action.IsInProgress || !action.IsAllyAction) continue;
            if (action.ActorCellId >= 0 && action.ActorCellId != localCellId) continue;
            currentAction = action;
            break;
        }

        return new ChampSelectSessionInfo
        {
            IsActive = true,
            Phase = phase,
            TimerMs = timerMs,
            Actions = actions,
            LocalPlayerCellId = localCellId,
            MyTeam = ParseChampSelectTeam(root, "myTeam"),
            TheirTeam = ParseChampSelectTeam(root, "theirTeam"),
            CurrentAction = currentAction,
        };
    }

    private static IReadOnlyList<ChampSelectTeamMember> ParseChampSelectTeam(JsonElement root, string key)
    {
        if (!root.TryGetProperty(key, out var arr) || arr.ValueKind != JsonValueKind.Array)
            return Array.Empty<ChampSelectTeamMember>();

        var list = new List<ChampSelectTeamMember>();
        foreach (var p in arr.EnumerateArray())
        {
            list.Add(new ChampSelectTeamMember
            {
                CellId = p.TryGetProperty("cellId", out var c) ? c.GetInt32() : 0,
                SummonerName = p.TryGetProperty("summonerName", out var sn) ? sn.GetString() ?? "" : "",
                AssignedPosition = p.TryGetProperty("assignedPosition", out var ap) ? ap.GetString() ?? "" : "",
                ChampionId = p.TryGetProperty("championId", out var cid) ? cid.GetInt32() : 0,
                ChampionPickIntent = p.TryGetProperty("championPickIntent", out var cpi) ? cpi.GetInt32() : 0,
            });
        }
        return list;
    }

    public async Task<bool> PatchChampSelectActionAsync(int actionId, int championId, bool completed = true) =>
        await PatchJsonAsync(
            $"/lol-champ-select/v1/session/actions/{actionId}",
            $"{{\"championId\":{championId},\"completed\":{completed.ToString().ToLowerInvariant()}}}");

    public async Task<bool> SetSummonerSpellsAsync(int spell1Id, int spell2Id) =>
        await PatchJsonAsync(
            "/lol-champ-select/v1/session/my-selection",
            $"{{\"spell1Id\":{spell1Id},\"spell2Id\":{spell2Id}}}");

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

    public async Task<PerkPageDetail?> GetCurrentPerkPageAsync()
    {
        using var doc = await GetJsonAsync("/lol-perks/v1/currentpage");
        if (doc is null) return null;
        return ParsePerkPageDetail(doc.RootElement);
    }

    public async Task<bool> UpdatePerkPageAsync(
        long pageId,
        string name,
        int primaryStyleId,
        int subStyleId,
        IReadOnlyList<int> selectedPerkIds,
        bool current = true)
    {
        var payload = new
        {
            id = pageId,
            name,
            primaryStyleId,
            subStyleId,
            selectedPerkIds,
            current,
        };
        var json = JsonSerializer.Serialize(payload);
        return await PutJsonAsync($"/lol-perks/v1/pages/{pageId}", json);
    }

    private static PerkPageDetail? ParsePerkPageDetail(JsonElement root)
    {
        if (root.ValueKind is JsonValueKind.Null or JsonValueKind.Undefined)
            return null;
        if (!root.TryGetProperty("id", out var idEl) || !idEl.TryGetInt64(out var pageId) || pageId <= 0)
            return null;

        var selected = new List<int>();
        if (root.TryGetProperty("selectedPerkIds", out var idsEl) && idsEl.ValueKind == JsonValueKind.Array)
        {
            foreach (var item in idsEl.EnumerateArray())
            {
                if (item.TryGetInt32(out var pid) && pid > 0)
                    selected.Add(pid);
            }
        }

        return new PerkPageDetail
        {
            Id = pageId,
            Name = root.TryGetProperty("name", out var nameEl) ? nameEl.GetString() ?? "" : "",
            PrimaryStyleId = root.TryGetProperty("primaryStyleId", out var pEl) ? pEl.GetInt32() : 0,
            SubStyleId = root.TryGetProperty("subStyleId", out var sEl) ? sEl.GetInt32() : 0,
            SelectedPerkIds = selected,
            IsCurrent = root.TryGetProperty("current", out var curEl) && curEl.GetBoolean(),
            IsDeletable = root.TryGetProperty("isDeletable", out var delEl) && delEl.GetBoolean(),
        };
    }

    public async Task<string?> GetGameflowPhaseAsync()
    {
        var doc = await GetJsonAsync("/lol-gameflow/v1/gameflow-phase");
        if (doc is null) return null;
        var phase = doc.RootElement.GetString();
        doc.Dispose();
        return phase;
    }

    public async Task<ParticipantStatusSnapshot> BuildParticipantStatusAsync()
    {
        var phase = await GetGameflowPhaseAsync() ?? "None";
        var lobby = await GetLobbyAsync();

        long? gameStartedAtMs = null;
        if (phase is "InProgress" or "PreEndOfGame")
            gameStartedAtMs = await GetGameStartedAtMsAsync();

        var status = MapParticipantStatus(phase, lobby);
        return new ParticipantStatusSnapshot(status, phase, gameStartedAtMs, true);
    }

    private static string MapParticipantStatus(string phase, LobbyInfo lobby) =>
        phase switch
        {
            "InProgress" or "PreEndOfGame" => "in_game",
            "ChampSelect" => "champ_select",
            "Lobby" when lobby.IsInLobby => "lobby",
            _ => "waiting",
        };

    private async Task<long?> GetGameStartedAtMsAsync()
    {
        using var doc = await GetJsonAsync("/lol-gameflow/v1/session");
        if (doc is null) return null;
        if (!doc.RootElement.TryGetProperty("gameData", out var gameData))
            return null;

        if (gameData.TryGetProperty("gameCreation", out var creationEl) &&
            creationEl.TryGetInt64(out var creationMs) && creationMs > 0)
            return creationMs;

        if (gameData.TryGetProperty("gameTime", out var gameTimeEl))
        {
            var seconds = gameTimeEl.GetDouble();
            if (seconds > 10_000)
                seconds /= 1000.0;
            if (seconds <= 0)
                return null;
            var startedMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() - (long)(seconds * 1000.0);
            return startedMs > 0 ? startedMs : null;
        }

        return null;
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

    public async Task<HashSet<string>> GetLobbyMemberRiotKeysAsync()
    {
        var keys = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var displayId in await GetLobbyMemberDisplayRiotIdsAsync())
            keys.Add(LcuPartyInvite.RiotKeyFromDisplay(displayId));
        return keys;
    }

    public async Task<IReadOnlyList<string>> GetLobbyMemberDisplayRiotIdsAsync()
    {
        var list = new List<string>();
        using var doc = await GetJsonAsync("/lol-lobby/v2/lobby");
        if (doc is null) return list;

        var root = doc.RootElement;
        if (root.ValueKind is JsonValueKind.Null or JsonValueKind.Undefined)
            return list;
        if (!root.TryGetProperty("members", out var members) || members.ValueKind != JsonValueKind.Array)
            return list;

        foreach (var member in members.EnumerateArray())
        {
            var gameName = ReadPlayerString(member, "riotIdGameName", "gameName", "summonerName");
            var tagLine = ReadPlayerString(member, "riotIdTagline", "riotIdTagLine", "gameTag", "tagLine");
            if (!string.IsNullOrWhiteSpace(gameName) && !string.IsNullOrWhiteSpace(tagLine))
                list.Add($"{gameName.Trim()}#{tagLine.Trim()}");
        }
        return list;
    }

    public static string? ReadLockfileSignature(string? lockfilePath)
    {
        if (string.IsNullOrWhiteSpace(lockfilePath) || !File.Exists(lockfilePath))
            return null;
        try
        {
            var raw = File.ReadAllText(lockfilePath).Trim();
            var parts = raw.Split(':');
            if (parts.Length < 4) return null;
            return $"{parts[2]}:{parts[3]}";
        }
        catch
        {
            return null;
        }
    }

    public async Task<(long? SummonerId, string? FailureReason)> ResolveSummonerIdForInviteAsync(
        string gameName, string tagLine)
    {
        var friends = await GetFriendsAsync();
        foreach (var friend in friends)
        {
            if (!string.Equals(friend.GameName, gameName, StringComparison.OrdinalIgnoreCase) ||
                !string.Equals(friend.TagLine, tagLine, StringComparison.OrdinalIgnoreCase))
                continue;
            if (!string.IsNullOrWhiteSpace(friend.Puuid))
            {
                var fromPuuid = await GetSummonerIdByPuuidAsync(friend.Puuid);
                if (fromPuuid is not null)
                    return (fromPuuid, null);
            }
        }

        var fromName = await ResolveSummonerIdByRiotIdAsync(gameName, tagLine);
        if (fromName is not null)
            return (fromName, null);

        var fromAccount = await ResolveSummonerIdFromAccountApiAsync(gameName, tagLine);
        if (fromAccount is not null)
            return (fromAccount, null);

        return (null, "닉 조회 실패 (Riot ID 확인 또는 친구 추가)");
    }

    public async Task<long?> ResolveSummonerIdByRiotIdAsync(string gameName, string tagLine)
    {
        var riotId = Uri.EscapeDataString($"{gameName}#{tagLine}");
        using var doc = await GetJsonAsync($"/lol-summoner/v1/summoners?name={riotId}");
        if (doc is null) return null;
        return TryReadSummonerId(doc.RootElement);
    }

    private async Task<long?> ResolveSummonerIdFromAccountApiAsync(string gameName, string tagLine)
    {
        var riotId = Uri.EscapeDataString($"{gameName}#{tagLine}");
        using var doc = await GetJsonAsync($"/lol-account/v1/accounts/aliases?riotId={riotId}");
        if (doc is null || doc.RootElement.ValueKind != JsonValueKind.Array)
            return null;

        foreach (var alias in doc.RootElement.EnumerateArray())
        {
            if (!alias.TryGetProperty("puuid", out var puuidEl))
                continue;
            var puuid = puuidEl.GetString();
            if (string.IsNullOrWhiteSpace(puuid))
                continue;
            var sid = await GetSummonerIdByPuuidAsync(puuid);
            if (sid is not null)
                return sid;
        }
        return null;
    }

    public async Task<long?> GetSummonerIdByPuuidAsync(string puuid)
    {
        if (string.IsNullOrWhiteSpace(puuid)) return null;
        using var doc = await GetJsonAsync($"/lol-summoner/v1/summoners-by-puuid-cached/{Uri.EscapeDataString(puuid)}");
        if (doc is null) return null;
        return TryReadSummonerId(doc.RootElement);
    }

    public async Task<bool> InviteToLobbyAsync(long summonerId) =>
        await PostJsonAsync("/lol-lobby/v2/lobby/invitations", $"[{{\"toSummonerId\":{summonerId}}}]");

    private static long? TryReadSummonerId(JsonElement root)
    {
        if (root.TryGetProperty("summonerId", out var sid) && sid.TryGetInt64(out var id))
            return id;
        return null;
    }

    public async Task<ReadyCheckInfo> GetReadyCheckAsync()
    {
        using var doc = await GetJsonAsync("/lol-matchmaking/v1/ready-check");
        if (doc is null) return ReadyCheckInfo.Inactive;

        var root = doc.RootElement;
        if (root.ValueKind is JsonValueKind.Null or JsonValueKind.Undefined)
            return ReadyCheckInfo.Inactive;

        var state = root.TryGetProperty("state", out var st) ? st.GetString() ?? "" : "";
        var playerResponse = root.TryGetProperty("playerResponse", out var pr)
            ? pr.GetString() ?? ""
            : "";
        var isActive = state is "InProgress" or "Waiting"
            && playerResponse is "" or "None" or "Pending";
        return new ReadyCheckInfo(isActive, state, playerResponse);
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

    public async Task<GuildMatchLcuPayload?> BuildGuildMatchEogPayloadAsync(string phase)
    {
        if (string.IsNullOrWhiteSpace(phase)) return null;

        using var eogDoc = await GetJsonAsync("/lol-end-of-game/v1/eog-stats-block");
        using var sessionDoc = await GetJsonAsync("/lol-gameflow/v1/session");

        var participants = new List<GuildMatchLcuParticipant>();
        string? reporterGameName = null;
        string? reporterTagLine = null;
        bool? reporterWon = null;

        if (eogDoc is not null)
        {
            ParseEogParticipants(eogDoc.RootElement, participants, ref reporterGameName, ref reporterTagLine, ref reporterWon);
        }

        if (participants.Count < 2 && sessionDoc is not null &&
            sessionDoc.RootElement.TryGetProperty("gameData", out var gameData))
        {
            ParseSessionParticipants(gameData, participants);
        }

        if (participants.Count < 2) return null;

        var gameResult = BuildGameResult(participants, reporterGameName, reporterTagLine, reporterWon);

        return new GuildMatchLcuPayload
        {
            GameflowPhase = phase,
            CapturedAt = DateTime.UtcNow.ToString("o"),
            Participants = participants,
            GameResult = gameResult,
            EogStats = eogDoc is null ? null : JsonSerializer.Deserialize<object>(eogDoc.RootElement.GetRawText())
        };
    }

    private static void ParseEogParticipants(
        JsonElement root,
        List<GuildMatchLcuParticipant> participants,
        ref string? reporterGameName,
        ref string? reporterTagLine,
        ref bool? reporterWon)
    {
        if (!root.TryGetProperty("teams", out var teams) || teams.ValueKind != JsonValueKind.Array)
            return;

        foreach (var team in teams.EnumerateArray())
        {
            var isWinningTeam = team.TryGetProperty("isWinningTeam", out var winEl) && winEl.GetBoolean();
            if (!team.TryGetProperty("players", out var players) || players.ValueKind != JsonValueKind.Array)
                continue;

            foreach (var player in players.EnumerateArray())
            {
                var gameName = ReadPlayerString(player, "riotIdGameName", "summonerName", "gameName");
                var tagLine = ReadPlayerString(player, "riotIdTagline", "riotIdTagLine", "tagLine");
                if (string.IsNullOrWhiteSpace(gameName) || string.IsNullOrWhiteSpace(tagLine))
                    continue;

                var teamId = player.TryGetProperty("teamId", out var teamEl) && teamEl.TryGetInt32(out var tid)
                    ? tid
                    : (int?)null;
                var isLocal = player.TryGetProperty("isLocalPlayer", out var localEl) && localEl.GetBoolean();
                var won = player.TryGetProperty("win", out var wonEl) && wonEl.ValueKind == JsonValueKind.True
                    ? true
                    : player.TryGetProperty("win", out var lostEl) && lostEl.ValueKind == JsonValueKind.False
                        ? false
                        : isWinningTeam;

                participants.Add(new GuildMatchLcuParticipant
                {
                    GameName = gameName,
                    TagLine = tagLine,
                    TeamId = teamId,
                    Won = won
                });

                if (isLocal)
                {
                    reporterGameName = gameName;
                    reporterTagLine = tagLine;
                    reporterWon = won;
                }
            }
        }
    }

    private static void ParseSessionParticipants(JsonElement gameData, List<GuildMatchLcuParticipant> participants)
    {
        foreach (var key in new[] { "teamOne", "teamTwo", "playerChampionSelections" })
        {
            if (!gameData.TryGetProperty(key, out var arr) || arr.ValueKind != JsonValueKind.Array)
                continue;

            foreach (var row in arr.EnumerateArray())
            {
                var gameName = ReadPlayerString(row, "riotIdGameName", "summonerName", "gameName");
                var tagLine = ReadPlayerString(row, "riotIdTagline", "riotIdTagLine", "tagLine");
                if (string.IsNullOrWhiteSpace(gameName) || string.IsNullOrWhiteSpace(tagLine))
                    continue;

                var teamId = row.TryGetProperty("teamId", out var teamEl) && teamEl.TryGetInt32(out var tid)
                    ? tid
                    : (int?)null;

                participants.Add(new GuildMatchLcuParticipant
                {
                    GameName = gameName,
                    TagLine = tagLine,
                    TeamId = teamId
                });
            }
        }
    }

    private static string ReadPlayerString(JsonElement row, params string[] keys)
    {
        foreach (var key in keys)
        {
            if (row.TryGetProperty(key, out var value) && value.ValueKind == JsonValueKind.String)
            {
                var text = value.GetString();
                if (!string.IsNullOrWhiteSpace(text))
                    return text.Trim();
            }
        }
        return "";
    }

    private static GuildMatchLcuGameResult? BuildGameResult(
        IReadOnlyList<GuildMatchLcuParticipant> participants,
        string? reporterGameName,
        string? reporterTagLine,
        bool? reporterWon)
    {
        if (reporterWon is not null)
        {
            return new GuildMatchLcuGameResult { DidWin = reporterWon };
        }

        var winners = participants.Where(p => p.Won == true).ToList();
        if (winners.Count == 0) return null;

        var winnerTeamId = winners[0].TeamId;
        if (winnerTeamId is 100) return new GuildMatchLcuGameResult { WinnerTeamSide = "blue" };
        if (winnerTeamId is 200) return new GuildMatchLcuGameResult { WinnerTeamSide = "red" };

        if (!string.IsNullOrWhiteSpace(reporterGameName) && !string.IsNullOrWhiteSpace(reporterTagLine))
        {
            var reporter = participants.FirstOrDefault(p =>
                string.Equals(p.GameName, reporterGameName, StringComparison.OrdinalIgnoreCase) &&
                string.Equals(p.TagLine, reporterTagLine, StringComparison.OrdinalIgnoreCase));
            if (reporter?.Won is not null)
                return new GuildMatchLcuGameResult { DidWin = reporter.Won };
        }

        return null;
    }

    public void Dispose() => _http.Dispose();
}
