using System.Text.Json;
using YummiLcu.Core.Lcu.Models;

namespace YummiLcu.Core.Lcu;

public readonly record struct ActionContext(LcuClient Lcu, AgentConfig Config, JsonElement? Payload);

public static class AllowedActions
{
    private const int MaxPartyInviteRiotIds = 20;

    private static readonly Dictionary<string, Func<ActionContext, Task<ActionResult>>> Handlers =
        new(StringComparer.Ordinal)
        {
            ["ping"] = _ => Task.FromResult(new ActionResult(true, "pong")),
            ["accept_match"] = async ctx => await Bool(await ctx.Lcu.PostAsync("/lol-matchmaking/v1/ready-check/accept")),
            ["decline_match"] = async ctx => await Bool(await ctx.Lcu.PostAsync("/lol-matchmaking/v1/ready-check/decline")),
            ["reconnect"] = async ctx => await Bool(await ctx.Lcu.PostAsync("/lol-gameflow/v1/reconnect")),
            ["dodge"] = DodgeAsync,
            ["queue_start"] = async ctx => await Bool(await ctx.Lcu.PostAsync("/lol-lobby/v2/lobby/matchmaking/search")),
            ["queue_cancel"] = async ctx => await Bool(await ctx.Lcu.DeleteAsync("/lol-lobby/v2/lobby/matchmaking/search")),
            ["leave_lobby"] = async ctx => await Bool(await ctx.Lcu.DeleteAsync("/lol-lobby/v2/lobby")),
            ["party_ready"] = async ctx => await Bool(await ctx.Lcu.PutJsonAsync("/lol-lobby/v1/parties/ready", """{"ready":true}""")),
            ["champ_reroll"] = async ctx => await Bool(await ctx.Lcu.PostAsync("/lol-champ-select/v1/session/my-selection/reroll")),
            ["champ_select_action"] = ChampSelectActionAsync,
            ["set_summoner_spells"] = SetSummonerSpellsAsync,
            ["list_rune_pages"] = ListRunePagesAsync,
            ["set_rune_page"] = SetRunePageAsync,
            ["get_current_rune_page"] = GetCurrentRunePageAsync,
            ["update_rune_page"] = UpdateRunePageAsync,
            ["quit_client"] = async ctx => await Bool(await ctx.Lcu.PostAsync("/process-control/v1/process/quit")),
            ["set_status"] = SetStatusAsync,
            ["reset_status"] = async ctx => await SetStatusTextAsync(ctx, StatusMessageHelper.DefaultYummiClient),
            ["claim_all_rewards"] = async ctx => new ActionResult(true, await LcuRewards.ClaimAllAsync(ctx.Lcu)),
            ["launch_client"] = _ => Task.FromResult(ToActionResult(LeagueLauncher.TryLaunch())),
            ["create_ranked_lobby"] = async ctx => ToActionResult(await LcuQueue.CreateLobbyAsync(ctx.Lcu, LcuQueue.RankedSolo)),
            ["create_normal_lobby"] = async ctx => ToActionResult(await LcuQueue.CreateLobbyAsync(ctx.Lcu, LcuQueue.NormalDraft)),
            ["play_ranked_solo"] = async ctx => ToActionResult(await LcuQueue.CreateAndQueueAsync(ctx.Lcu, LcuQueue.RankedSolo)),
            ["play_normal_draft"] = async ctx => ToActionResult(await LcuQueue.CreateAndQueueAsync(ctx.Lcu, LcuQueue.NormalDraft)),
            ["invite_party_members"] = InvitePartyMembersAsync,
            ["check_party_members"] = CheckPartyMembersAsync,
        };

    public static IReadOnlyCollection<string> Names => Handlers.Keys;
    public static bool IsAllowed(string action) => Handlers.ContainsKey(action);

    public static async Task<ActionResult> ExecuteAsync(string action, ActionContext ctx)
    {
        if (!Handlers.TryGetValue(action, out var fn))
            return new ActionResult(false, "unknown action");
        return await fn(ctx);
    }

    private static Task<ActionResult> Bool(bool ok) =>
        Task.FromResult(ActionResult.FromBool(ok));

    private static ActionResult ToActionResult((bool Ok, string Message) result) =>
        new(result.Ok, result.Message);

    private static async Task<ActionResult> DodgeAsync(ActionContext ctx)
    {
        var ok = await ctx.Lcu.PostAsync("/lol-gameflow/v1/session/dodge");
        if (!ok) return new ActionResult(false, "닷지 실패");
        if (ctx.Config.PreventQueueAfterDodge)
        {
            await ctx.Lcu.DeleteAsync("/lol-lobby/v2/lobby/matchmaking/search");
            return new ActionResult(true, "닷지 + 매칭 중지");
        }
        return new ActionResult(true, "닷지 완료");
    }

    private static async Task<ActionResult> SetStatusAsync(ActionContext ctx)
    {
        var text = PayloadString(ctx.Payload, "text");
        return await SetStatusTextAsync(ctx, text);
    }

    private static async Task<ActionResult> SetStatusTextAsync(ActionContext ctx, string? raw)
    {
        var text = StatusMessageHelper.Normalize(raw);
        if (!StatusMessageHelper.TryValidate(text, out var err))
            return new ActionResult(false, err);
        var ok = await ctx.Lcu.SetStatusMessageAsync(text);
        return ok
            ? new ActionResult(true, $"상메 설정: {text[..Math.Min(text.Length, 40)]}")
            : new ActionResult(false, "상메 설정 실패");
    }

    private static async Task<ActionResult> ChampSelectActionAsync(ActionContext ctx)
    {
        var actionId = PayloadInt(ctx.Payload, "action_id");
        var championId = PayloadInt(ctx.Payload, "champion_id");
        if (actionId is null or < 0)
            return new ActionResult(false, "action_id가 필요합니다.");
        if (championId is null or <= 0)
            return new ActionResult(false, "champion_id가 필요합니다.");
        var ok = await ctx.Lcu.PatchChampSelectActionAsync(actionId.Value, championId.Value);
        return ActionResult.FromBool(ok);
    }

    private static async Task<ActionResult> SetSummonerSpellsAsync(ActionContext ctx)
    {
        var spell1Id = PayloadInt(ctx.Payload, "spell1_id");
        var spell2Id = PayloadInt(ctx.Payload, "spell2_id");
        if (spell1Id is null or <= 0 || spell2Id is null or <= 0)
            return new ActionResult(false, "spell1_id/spell2_id가 필요합니다.");
        if (spell1Id == spell2Id)
            return new ActionResult(false, "서로 다른 스펠을 선택하세요.");
        var ok = await ctx.Lcu.SetSummonerSpellsAsync(spell1Id.Value, spell2Id.Value);
        return ActionResult.FromBool(ok, "스펠 변경 완료", "스펠 변경 실패");
    }

    private static async Task<ActionResult> ListRunePagesAsync(ActionContext ctx)
    {
        var pages = await ctx.Lcu.GetPerkPagesAsync();
        var data = JsonSerializer.SerializeToElement(new
        {
            pages = pages
                .Where(p => p.Id > 0)
                .Select(p => new { id = p.Id, name = p.Name, current = p.IsActive })
                .Take(25)
                .ToList(),
        });
        return new ActionResult(true, $"{pages.Count}개 룬 페이지", data);
    }

    private static async Task<ActionResult> SetRunePageAsync(ActionContext ctx)
    {
        var pageId = PayloadLong(ctx.Payload, "page_id");
        if (pageId is null or <= 0)
            return new ActionResult(false, "page_id가 필요합니다.");
        var ok = await ctx.Lcu.SetCurrentPerkPageAsync(pageId.Value);
        return ActionResult.FromBool(ok, "룬 페이지 변경 완료", "룬 페이지 변경 실패");
    }

    private static async Task<ActionResult> GetCurrentRunePageAsync(ActionContext ctx)
    {
        var page = await ctx.Lcu.GetCurrentPerkPageAsync();
        if (page is null)
            return new ActionResult(false, "현재 룬 페이지를 읽지 못했습니다.");
        var data = JsonSerializer.SerializeToElement(new
        {
            id = page.Id,
            name = page.Name,
            primary_style_id = page.PrimaryStyleId,
            sub_style_id = page.SubStyleId,
            selected_perk_ids = page.SelectedPerkIds,
            current = page.IsCurrent,
        });
        return new ActionResult(true, "현재 룬 페이지", data);
    }

    private static async Task<ActionResult> UpdateRunePageAsync(ActionContext ctx)
    {
        var pageId = PayloadLong(ctx.Payload, "page_id");
        var name = PayloadString(ctx.Payload, "name");
        var primaryStyleId = PayloadInt(ctx.Payload, "primary_style_id");
        var subStyleId = PayloadInt(ctx.Payload, "sub_style_id");
        var perkIds = PayloadIntArray(ctx.Payload, "selected_perk_ids");
        if (pageId is null or <= 0)
            return new ActionResult(false, "page_id가 필요합니다.");
        if (primaryStyleId is null or <= 0 || subStyleId is null or <= 0)
            return new ActionResult(false, "primary_style_id/sub_style_id가 필요합니다.");
        if (perkIds.Count != 9)
            return new ActionResult(false, "selected_perk_ids는 9개여야 합니다.");

        var ok = await ctx.Lcu.UpdatePerkPageAsync(
            pageId.Value,
            string.IsNullOrWhiteSpace(name) ? "Yummi" : name.Trim(),
            primaryStyleId.Value,
            subStyleId.Value,
            perkIds,
            current: true);
        return ActionResult.FromBool(ok, "룬 구성 저장 완료", "룬 구성 저장 실패");
    }

    private static List<int> PayloadIntArray(JsonElement? payload, string key)
    {
        if (payload is null || payload.Value.ValueKind != JsonValueKind.Object)
            return new List<int>();
        if (!payload.Value.TryGetProperty(key, out var el) || el.ValueKind != JsonValueKind.Array)
            return new List<int>();

        var list = new List<int>();
        foreach (var item in el.EnumerateArray())
        {
            if (item.ValueKind == JsonValueKind.Number && item.TryGetInt32(out var n) && n > 0)
                list.Add(n);
            else if (item.ValueKind == JsonValueKind.String && int.TryParse(item.GetString(), out var parsed) && parsed > 0)
                list.Add(parsed);
        }
        return list;
    }

    private static int? PayloadInt(JsonElement? payload, string key)
    {
        if (payload is null || payload.Value.ValueKind != JsonValueKind.Object) return null;
        if (!payload.Value.TryGetProperty(key, out var el)) return null;
        return el.ValueKind switch
        {
            JsonValueKind.Number when el.TryGetInt32(out var n) => n,
            JsonValueKind.String when int.TryParse(el.GetString(), out var parsed) => parsed,
            _ => null,
        };
    }

    private static string? PayloadString(JsonElement? payload, string key)
    {
        if (payload is null || payload.Value.ValueKind != JsonValueKind.Object) return null;
        if (!payload.Value.TryGetProperty(key, out var el)) return null;
        return el.GetString();
    }

    private static long? PayloadLong(JsonElement? payload, string key)
    {
        if (payload is null || payload.Value.ValueKind != JsonValueKind.Object) return null;
        if (!payload.Value.TryGetProperty(key, out var el)) return null;
        return el.ValueKind switch
        {
            JsonValueKind.Number when el.TryGetInt64(out var n) => n,
            JsonValueKind.String when long.TryParse(el.GetString(), out var parsed) => parsed,
            _ => null,
        };
    }

    private static IReadOnlyList<string> PayloadStringArray(JsonElement? payload, string key)
    {
        if (payload is null || payload.Value.ValueKind != JsonValueKind.Object)
            return Array.Empty<string>();
        if (!payload.Value.TryGetProperty(key, out var el) || el.ValueKind != JsonValueKind.Array)
            return Array.Empty<string>();

        var list = new List<string>();
        foreach (var item in el.EnumerateArray())
        {
            if (item.ValueKind == JsonValueKind.String)
            {
                var s = item.GetString();
                if (!string.IsNullOrWhiteSpace(s))
                    list.Add(s.Trim());
            }
        }
        return list;
    }

    private static async Task<ActionResult> InvitePartyMembersAsync(ActionContext ctx)
    {
        var lobby = await ctx.Lcu.GetLobbyAsync();
        if (!lobby.IsInLobby)
            return new ActionResult(false, "로비(파티)가 열려 있지 않습니다.");

        var riotIds = PayloadStringArray(ctx.Payload, "riot_ids");
        var checkIds = PayloadStringArray(ctx.Payload, "check_riot_ids");
        if (checkIds.Count == 0)
            checkIds = riotIds;
        if (riotIds.Count == 0 && checkIds.Count == 0)
            return new ActionResult(false, "초대할 Riot ID가 없습니다.");
        if (riotIds.Count > MaxPartyInviteRiotIds)
            return new ActionResult(false, $"초대는 최대 {MaxPartyInviteRiotIds}명까지 가능합니다.");

        var inParty = await ctx.Lcu.GetLobbyMemberRiotKeysAsync();
        var invited = 0;
        var failed = 0;
        var details = new List<string>();
        var memberStatuses = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);

        foreach (var raw in riotIds)
        {
            if (!LcuPartyInvite.TryParseRiotId(raw, out var gameName, out var tagLine))
            {
                failed++;
                memberStatuses[raw] = "invite_failed";
                details.Add($"{raw}: Riot ID 형식 오류");
                continue;
            }

            var key = LcuPartyInvite.RiotKey(gameName, tagLine);
            var displayId = $"{gameName}#{tagLine}";
            if (inParty.Contains(key))
            {
                memberStatuses[displayId] = "in_party";
                continue;
            }

            var (summonerId, lookupError) = await ctx.Lcu.ResolveSummonerIdForInviteAsync(gameName, tagLine);
            if (summonerId is null)
            {
                failed++;
                memberStatuses[displayId] = "invite_failed";
                details.Add($"{displayId}: {lookupError ?? "닉 조회 실패"}");
                continue;
            }

            if (await ctx.Lcu.InviteToLobbyAsync(summonerId.Value))
            {
                invited++;
                memberStatuses[displayId] = "invited";
            }
            else
            {
                failed++;
                memberStatuses[displayId] = "invite_failed";
                details.Add($"{displayId}: 초대 실패");
            }

            await Task.Delay(250);
        }

        inParty = await ctx.Lcu.GetLobbyMemberRiotKeysAsync();
        foreach (var raw in checkIds)
        {
            if (!LcuPartyInvite.TryParseRiotId(raw, out var gameName, out var tagLine))
                continue;
            var displayId = $"{gameName}#{tagLine}";
            var key = LcuPartyInvite.RiotKey(gameName, tagLine);
            if (inParty.Contains(key))
                memberStatuses[displayId] = "in_party";
        }

        var members = memberStatuses
            .Select(kv => new { riot_id = kv.Key, status = kv.Value })
            .ToList();
        var allInParty = checkIds.Count > 0 && checkIds.All(raw =>
        {
            if (!LcuPartyInvite.TryParseRiotId(raw, out var gn, out var tl))
                return false;
            return inParty.Contains(LcuPartyInvite.RiotKey(gn, tl));
        });

        var summary = $"파티 참가 {members.Count(m => m.status == "in_party")}명, 초대됨 {members.Count(m => m.status == "invited")}명, 실패 {members.Count(m => m.status == "invite_failed")}명";
        if (details.Count > 0 && details.Count <= 6)
            summary += "\n" + string.Join("\n", details);

        var data = JsonSerializer.SerializeToElement(new { members, all_in_party = allInParty });
        var ok = allInParty || invited > 0 || members.Any(m => m.status is "in_party" or "invited");
        if (!ok && failed > 0)
            return new ActionResult(false, summary, data);
        return new ActionResult(true, summary, data);
    }

    private static async Task<ActionResult> CheckPartyMembersAsync(ActionContext ctx)
    {
        var lobby = await ctx.Lcu.GetLobbyAsync();
        if (!lobby.IsInLobby)
            return new ActionResult(false, "로비(파티)가 열려 있지 않습니다.");

        var checkIds = PayloadStringArray(ctx.Payload, "check_riot_ids");
        if (checkIds.Count == 0)
            return new ActionResult(false, "확인할 Riot ID가 없습니다.");
        if (checkIds.Count > MaxPartyInviteRiotIds)
            return new ActionResult(false, $"확인은 최대 {MaxPartyInviteRiotIds}명까지 가능합니다.");

        var inParty = await ctx.Lcu.GetLobbyMemberRiotKeysAsync();
        var (members, allInParty, summary) = BuildPartyCheckResult(checkIds, inParty);
        var data = JsonSerializer.SerializeToElement(new { members, all_in_party = allInParty });
        return new ActionResult(true, summary, data);
    }

    private static (List<object> Members, bool AllInParty, string Summary) BuildPartyCheckResult(
        IReadOnlyList<string> checkIds,
        HashSet<string> inParty)
    {
        var members = new List<object>();
        foreach (var raw in checkIds)
        {
            if (!LcuPartyInvite.TryParseRiotId(raw, out var gameName, out var tagLine))
                continue;
            var displayId = $"{gameName}#{tagLine}";
            var key = LcuPartyInvite.RiotKey(gameName, tagLine);
            if (inParty.Contains(key))
                members.Add(new { riot_id = displayId, status = "in_party" });
        }

        var allInParty = checkIds.All(raw =>
        {
            if (!LcuPartyInvite.TryParseRiotId(raw, out var gn, out var tl))
                return false;
            return inParty.Contains(LcuPartyInvite.RiotKey(gn, tl));
        });
        var summary = $"파티 참가 {members.Count}/{checkIds.Count}명";
        return (members, allInParty, summary);
    }
}
