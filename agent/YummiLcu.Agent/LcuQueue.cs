namespace YummiLcu.Agent;

/// <summary>로비 생성 + 매칭 시작 (클라 홈의 솔랭·일반 비공개 선택과 동일 queueId).</summary>
internal static class LcuQueue
{
    /// <summary>솔로 랭크.</summary>
    public const int RankedSolo = 420;

    /// <summary>일반 게임 — 비공개 선택 (드래프트).</summary>
    public const int NormalDraft = 400;

    public static async Task<(bool Ok, string Message)> CreateAndQueueAsync(LcuClient lcu, int queueId, int maxAttempts = 6)
    {
        for (var attempt = 1; attempt <= maxAttempts; attempt++)
        {
            await lcu.DeleteAsync("/lol-lobby/v2/lobby/matchmaking/search");
            await lcu.DeleteAsync("/lol-lobby/v2/lobby");

            var lobbyOk = await lcu.PostJsonAsync("/lol-lobby/v2/lobby", $"{{\"queueId\":{queueId}}}");
            if (!lobbyOk)
            {
                if (attempt < maxAttempts)
                {
                    await Task.Delay(2500);
                    continue;
                }
                return (false, $"로비 생성 실패 (queue {queueId})");
            }

            await Task.Delay(400);
            var searchOk = await lcu.PostAsync("/lol-lobby/v2/lobby/matchmaking/search");
            if (searchOk)
            {
                var label = queueId == RankedSolo ? "솔랭" : queueId == NormalDraft ? "일반(비공개)" : $"queue {queueId}";
                return (true, $"{label} 매칭 시작");
            }

            if (attempt < maxAttempts)
                await Task.Delay(2500);
        }

        return (false, "매칭 시작 실패");
    }
}
