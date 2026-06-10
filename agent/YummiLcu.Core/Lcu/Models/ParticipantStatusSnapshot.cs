namespace YummiLcu.Core.Lcu.Models;

/// <summary>모집창 등에서 표시할 참가자 LCU 상태 스냅샷.</summary>
public sealed record ParticipantStatusSnapshot(
    string Status,
    string? Phase,
    long? GameStartedAtMs,
    bool LcuReady)
{
    public static ParticipantStatusSnapshot Offline() =>
        new("offline", "None", null, false);

    public static ParticipantStatusSnapshot WaitingWithoutLcu() =>
        new("waiting", "None", null, false);
}
