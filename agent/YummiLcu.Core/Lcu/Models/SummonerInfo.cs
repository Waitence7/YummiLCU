namespace YummiLcu.Core.Lcu.Models;

public sealed class SummonerInfo
{
    public string DisplayName { get; init; } = "";
    public long SummonerId { get; init; }
    public int Level { get; init; }
    public int ProfileIconId { get; init; }
}
