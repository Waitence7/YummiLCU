namespace YummiLcu.Core.Lcu.Models;

public sealed class PerkPageDetail
{
    public long Id { get; init; }
    public string Name { get; init; } = "";
    public int PrimaryStyleId { get; init; }
    public int SubStyleId { get; init; }
    public IReadOnlyList<int> SelectedPerkIds { get; init; } = Array.Empty<int>();
    public bool IsCurrent { get; init; }
    public bool IsDeletable { get; init; }
}
