namespace YummiLcu.Core.Lcu.Models;

public readonly record struct ReadyCheckInfo(bool IsActive, string State, string PlayerResponse)
{
    public static ReadyCheckInfo Inactive { get; } = new(false, "", "");
}
