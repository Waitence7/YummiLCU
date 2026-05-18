using System.Text.Json;

namespace YummiLcu.Agent;

internal static class LcuRewards
{
    public static async Task<string> ClaimAllAsync(LcuClient lcu)
    {
        var lines = new List<string>();
        var ok = 0;

        ok += await ClaimLootNotificationsAsync(lcu, lines);
        ok += await ClaimMilestonesAsync(lcu, lines);
        ok += await ClaimMissionsAsync(lcu, lines);
        ok += await ClaimEventHubAsync(lcu, lines);
        ok += await RedeemPlayerLootAsync(lcu, lines);

        if (lines.Count == 0)
            return "수령할 보상을 찾지 못했습니다. (클라 보상 센터를 한 번 열어 보세요)";
        return $"보상 처리 완료 (성공 항목 {ok}건)\n" + string.Join("\n", lines.Take(12));
    }

    private static async Task<int> ClaimLootNotificationsAsync(LcuClient lcu, List<string> lines)
    {
        var doc = await lcu.GetJsonAsync("/lol-loot/v1/player-loot-notifications");
        if (doc is null)
            return 0;
        var n = 0;
        if (doc.RootElement.ValueKind == JsonValueKind.Array)
        {
            foreach (var item in doc.RootElement.EnumerateArray())
            {
                if (!item.TryGetProperty("id", out var idEl))
                    continue;
                var id = idEl.GetString();
                if (string.IsNullOrEmpty(id))
                    continue;
                if (await lcu.PostAsync($"/lol-loot/v1/player-loot-notifications/{Uri.EscapeDataString(id)}/acknowledge"))
                {
                    n++;
                    lines.Add($"알림 보상: {id[..Math.Min(id.Length, 20)]}");
                }
            }
        }
        doc.Dispose();
        return n;
    }

    private static async Task<int> ClaimMilestonesAsync(LcuClient lcu, List<string> lines)
    {
        var doc = await lcu.GetJsonAsync("/lol-loot/v1/milestones");
        if (doc is null)
            return 0;
        var n = 0;
        if (doc.RootElement.ValueKind == JsonValueKind.Array)
        {
            foreach (var ms in doc.RootElement.EnumerateArray())
            {
                if (!ms.TryGetProperty("id", out var idEl))
                    continue;
                var id = idEl.GetString();
                if (string.IsNullOrEmpty(id))
                    continue;
                if (await lcu.PostAsync($"/lol-loot/v1/milestones/{Uri.EscapeDataString(id)}/claim"))
                {
                    n++;
                    lines.Add($"마일스톤: {id}");
                }
            }
        }
        doc.Dispose();
        return n;
    }

    private static async Task<int> ClaimMissionsAsync(LcuClient lcu, List<string> lines)
    {
        var doc = await lcu.GetJsonAsync("/lol-missions/v1/missions");
        if (doc is null)
            return 0;
        var n = 0;
        if (doc.RootElement.ValueKind == JsonValueKind.Array)
        {
            foreach (var m in doc.RootElement.EnumerateArray())
            {
                if (!m.TryGetProperty("id", out var idEl))
                    continue;
                var id = idEl.GetString();
                if (string.IsNullOrEmpty(id))
                    continue;
                var status = m.TryGetProperty("status", out var st) ? st.GetString() : "";
                if (status is not ("COMPLETED" or "COMPLETE"))
                    continue;
                if (await lcu.PutJsonAsync($"/lol-missions/v1/player/{Uri.EscapeDataString(id)}", "{}"))
                {
                    n++;
                    lines.Add($"미션: {id[..Math.Min(id.Length, 24)]}");
                }
            }
        }
        doc.Dispose();
        return n;
    }

    private static async Task<int> ClaimEventHubAsync(LcuClient lcu, List<string> lines)
    {
        var hub = await lcu.GetJsonAsync("/lol-event-hub/v1/events");
        if (hub is null)
            return 0;
        var n = 0;
        if (hub.RootElement.ValueKind == JsonValueKind.Array)
        {
            foreach (var ev in hub.RootElement.EnumerateArray())
            {
                if (!ev.TryGetProperty("eventId", out var idEl))
                    continue;
                var eventId = idEl.GetString();
                if (string.IsNullOrEmpty(eventId))
                    continue;
                if (await lcu.PostAsync($"/lol-event-hub/v1/events/{Uri.EscapeDataString(eventId)}/reward-track/claim-all"))
                {
                    n++;
                    lines.Add($"이벤트 허브: {eventId}");
                }
            }
        }
        hub.Dispose();
        return n;
    }

    private static async Task<int> RedeemPlayerLootAsync(LcuClient lcu, List<string> lines)
    {
        var doc = await lcu.GetJsonAsync("/lol-loot/v1/player-loot");
        if (doc is null)
            return 0;
        var n = 0;
        if (doc.RootElement.ValueKind == JsonValueKind.Array)
        {
            foreach (var loot in doc.RootElement.EnumerateArray())
            {
                var redeemable = loot.TryGetProperty("redeemableStatus", out var rs) && rs.GetString() == "REDEEMABLE";
                if (!redeemable && loot.TryGetProperty("isRevealed", out var rev) && !rev.GetBoolean())
                    continue;
                if (!loot.TryGetProperty("lootName", out var nameEl))
                    continue;
                var lootName = nameEl.GetString();
                if (string.IsNullOrEmpty(lootName))
                    continue;
                if (await lcu.PostAsync($"/lol-loot/v1/player-loot/{Uri.EscapeDataString(lootName)}/redeem"))
                {
                    n++;
                    if (lines.Count < 8)
                        lines.Add($"루트: {lootName}");
                }
            }
        }
        doc.Dispose();
        return n;
    }
}
