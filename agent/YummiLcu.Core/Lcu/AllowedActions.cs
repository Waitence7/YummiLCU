using System.Text.Json;
using YummiLcu.Core.Lcu.Models;

namespace YummiLcu.Core.Lcu;

public readonly record struct ActionContext(LcuClient Lcu, AgentConfig Config, JsonElement? Payload);

public static class AllowedActions
{
    private static readonly Dictionary<string, Func<ActionContext, Task<(bool Ok, string Message)>>> Handlers =
        new(StringComparer.Ordinal)
        {
            ["ping"] = _ => Task.FromResult((true, "pong")),
            ["accept_match"] = async ctx => await Bool(await ctx.Lcu.PostAsync("/lol-matchmaking/v1/ready-check/accept")),
            ["decline_match"] = async ctx => await Bool(await ctx.Lcu.PostAsync("/lol-matchmaking/v1/ready-check/decline")),
            ["reconnect"] = async ctx => await Bool(await ctx.Lcu.PostAsync("/lol-gameflow/v1/reconnect")),
            ["dodge"] = DodgeAsync,
            ["queue_start"] = async ctx => await Bool(await ctx.Lcu.PostAsync("/lol-lobby/v2/lobby/matchmaking/search")),
            ["queue_cancel"] = async ctx => await Bool(await ctx.Lcu.DeleteAsync("/lol-lobby/v2/lobby/matchmaking/search")),
            ["leave_lobby"] = async ctx => await Bool(await ctx.Lcu.DeleteAsync("/lol-lobby/v2/lobby")),
            ["party_ready"] = async ctx => await Bool(await ctx.Lcu.PutJsonAsync("/lol-lobby/v1/parties/ready", """{"ready":true}""")),
            ["champ_reroll"] = async ctx => await Bool(await ctx.Lcu.PostAsync("/lol-champ-select/v1/session/my-selection/reroll")),
            ["quit_client"] = async ctx => await Bool(await ctx.Lcu.PostAsync("/process-control/v1/process/quit")),
            ["set_status"] = SetStatusAsync,
            ["reset_status"] = async ctx => await SetStatusTextAsync(ctx, StatusMessageHelper.DefaultYummiClient),
            ["claim_all_rewards"] = async ctx => (true, await LcuRewards.ClaimAllAsync(ctx.Lcu)),
            ["launch_client"] = _ => Task.FromResult(LeagueLauncher.TryLaunch()),
            ["create_ranked_lobby"] = async ctx => await LcuQueue.CreateLobbyAsync(ctx.Lcu, LcuQueue.RankedSolo),
            ["create_normal_lobby"] = async ctx => await LcuQueue.CreateLobbyAsync(ctx.Lcu, LcuQueue.NormalDraft),
            ["play_ranked_solo"] = async ctx => await LcuQueue.CreateAndQueueAsync(ctx.Lcu, LcuQueue.RankedSolo),
            ["play_normal_draft"] = async ctx => await LcuQueue.CreateAndQueueAsync(ctx.Lcu, LcuQueue.NormalDraft),
            ["invite_party_members"] = InvitePartyMembersAsync,
        };

    public static IReadOnlyCollection<string> Names => Handlers.Keys;
    public static bool IsAllowed(string action) => Handlers.ContainsKey(action);

    public static async Task<(bool Ok, string Message)> ExecuteAsync(string action, ActionContext ctx)
    {
        if (!Handlers.TryGetValue(action, out var fn))
            return (false, "unknown action");
        return await fn(ctx);
    }

    private static Task<(bool Ok, string Message)> Bool(bool ok) =>
        Task.FromResult((ok, ok ? "ok" : "LCU 요청 실패"));

    private static async Task<(bool Ok, string Message)> DodgeAsync(ActionContext ctx)
    {
        var ok = await ctx.Lcu.PostAsync("/lol-gameflow/v1/session/dodge");
        if (!ok) return (false, "닷지 실패");
        if (ctx.Config.PreventQueueAfterDodge)
        {
            await ctx.Lcu.DeleteAsync("/lol-lobby/v2/lobby/matchmaking/search");
            return (true, "닷지 + 매칭 중지");
        }
        return (true, "닷지 완료");
    }

    private static async Task<(bool Ok, string Message)> SetStatusAsync(ActionContext ctx)
    {
        var text = PayloadString(ctx.Payload, "text");
        return await SetStatusTextAsync(ctx, text);
    }

    private static async Task<(bool Ok, string Message)> SetStatusTextAsync(ActionContext ctx, string? raw)
    {
        var text = StatusMessageHelper.Normalize(raw);
        if (!StatusMessageHelper.TryValidate(text, out var err))
            return (false, err);
        var ok = await ctx.Lcu.SetStatusMessageAsync(text);
        return ok ? (true, $"상메 설정: {text[..Math.Min(text.Length, 40)]}") : (false, "상메 설정 실패");
    }

    private static string? PayloadString(JsonElement? payload, string key)
    {
        if (payload is null || payload.Value.ValueKind != JsonValueKind.Object) return null;
        if (!payload.Value.TryGetProperty(key, out var el)) return null;
        return el.GetString();
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

    private static async Task<(bool Ok, string Message)> InvitePartyMembersAsync(ActionContext ctx)
    {
        var lobby = await ctx.Lcu.GetLobbyAsync();
        if (!lobby.IsInLobby)
            return (false, "로비(파티)가 열려 있지 않습니다.");

        var riotIds = PayloadStringArray(ctx.Payload, "riot_ids");
        if (riotIds.Count == 0)
            return (false, "초대할 Riot ID가 없습니다.");

        var inParty = await ctx.Lcu.GetLobbyMemberRiotKeysAsync();
        var invited = 0;
        var skipped = 0;
        var failed = 0;
        var details = new List<string>();

        foreach (var raw in riotIds)
        {
            if (!LcuPartyInvite.TryParseRiotId(raw, out var gameName, out var tagLine))
            {
                failed++;
                details.Add($"{raw}: Riot ID 형식 오류");
                continue;
            }

            var key = LcuPartyInvite.RiotKey(gameName, tagLine);
            if (inParty.Contains(key))
            {
                skipped++;
                continue;
            }

            var summonerId = await ctx.Lcu.ResolveSummonerIdByRiotIdAsync(gameName, tagLine);
            if (summonerId is null)
            {
                failed++;
                details.Add($"{gameName}#{tagLine}: 닉 조회 실패");
                continue;
            }

            if (await ctx.Lcu.InviteToLobbyAsync(summonerId.Value))
            {
                invited++;
                inParty.Add(key);
            }
            else
            {
                failed++;
                details.Add($"{gameName}#{tagLine}: 초대 실패");
            }

            await Task.Delay(250);
        }

        var summary = $"초대 {invited}명, 스킵 {skipped}명(파티 중), 실패 {failed}명";
        if (details.Count > 0 && details.Count <= 6)
            summary += "\n" + string.Join("\n", details);

        var ok = invited > 0 || (skipped > 0 && failed == 0);
        if (!ok && failed > 0 && invited == 0 && skipped == 0)
            return (false, summary);
        return (true, summary);
    }
}
