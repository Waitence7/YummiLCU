namespace YummiLcu.Core.Lcu.Models;

public sealed class ChampSelectSessionInfo
{
    public bool IsActive { get; init; }
    public string Phase { get; init; } = "";
    public int TimerMs { get; init; }
    public IReadOnlyList<ChampSelectAction> Actions { get; init; } = Array.Empty<ChampSelectAction>();
    public int LocalPlayerCellId { get; init; }
    public IReadOnlyList<ChampSelectTeamMember> MyTeam { get; init; } = Array.Empty<ChampSelectTeamMember>();
    public IReadOnlyList<ChampSelectTeamMember> TheirTeam { get; init; } = Array.Empty<ChampSelectTeamMember>();
    public ChampSelectAction? CurrentAction { get; init; }
}
