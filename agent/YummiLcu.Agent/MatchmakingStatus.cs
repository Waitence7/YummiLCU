namespace YummiLcu.Agent;

/// <summary>LCU 매칭 검색 상태 (GET /lol-matchmaking/v1/search).</summary>
internal readonly record struct MatchmakingStatus(
    bool IsSearching,
    double TimeInQueueSeconds,
    double EstimatedQueueTimeSeconds)
{
    public static MatchmakingStatus Idle { get; } = new(false, 0, 0);

    public string DisplayLine =>
        IsSearching
            ? $"매칭 {FormatDuration(TimeInQueueSeconds)} · 예상 {FormatDuration(EstimatedQueueTimeSeconds)}"
            : "";

    public static string FormatDuration(double rawSeconds)
    {
        var sec = (int)Math.Round(NormalizeSeconds(rawSeconds));
        if (sec < 0)
            sec = 0;
        var m = sec / 60;
        var s = sec % 60;
        return m > 0 ? $"{m}분 {s}초" : $"{s}초";
    }

    internal static double NormalizeSeconds(double value)
    {
        if (double.IsNaN(value) || value < 0)
            return 0;
        // 일부 빌드에서 ms로 오는 경우
        if (value > 86_400)
            return value / 1000.0;
        return value;
    }
}
