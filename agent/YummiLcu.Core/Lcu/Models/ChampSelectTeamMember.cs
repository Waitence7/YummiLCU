namespace YummiLcu.Core.Lcu.Models;

public sealed class ChampSelectTeamMember
{
    public int CellId { get; init; }
    public string SummonerName { get; init; } = "";
    public string AssignedPosition { get; init; } = "";
    public int ChampionId { get; init; }
    public int ChampionPickIntent { get; init; }
}
