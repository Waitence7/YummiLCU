namespace YummiLcu.Core.Lcu.Models;

public readonly record struct LobbyInfo(
    bool IsInLobby,
    int QueueId,
    string QueueLabel,
    int MemberCount,
    int MaxMembers)
{
    public static LobbyInfo None { get; } = new(false, 0, "", 0, 5);

    public static string LabelForQueue(int queueId) => queueId switch
    {
        LcuQueue.RankedSolo => "솔로/듀오 랭크",
        LcuQueue.NormalDraft => "일반 (비공개)",
        LcuQueue.Aram => "무작위 총력전",
        440 => "자유 랭크",
        _ => queueId > 0 ? $"큐 {queueId}" : "로비",
    };
}
