namespace YummiLcu.Core.Lcu.Models;

public sealed class ChampSelectAction
{
    public int Id { get; init; }
    public string Type { get; init; } = "";
    public int ChampionId { get; init; }
    public bool Completed { get; init; }
    public bool IsAllyAction { get; init; }
    public bool IsInProgress { get; init; }
    public int ActorCellId { get; init; }
}
