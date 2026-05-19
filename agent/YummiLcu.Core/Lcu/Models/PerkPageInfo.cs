namespace YummiLcu.Core.Lcu.Models;

public sealed class PerkPageInfo
{
    public long Id { get; init; }
    public string Name { get; init; } = "";
    public bool IsActive { get; init; }
    public bool IsDeletable { get; init; }
}
