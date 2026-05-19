namespace YummiLcu.Core.Lcu.Models;

public sealed class ChampSelectSessionInfo
{
    public bool IsActive { get; init; }
    public string Phase { get; init; } = "";
    public IReadOnlyList<ChampSelectAction> Actions { get; init; } = Array.Empty<ChampSelectAction>();
    public int LocalPlayerCellId { get; init; }
}
