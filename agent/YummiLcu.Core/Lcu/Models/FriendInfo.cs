namespace YummiLcu.Core.Lcu.Models;

public sealed class FriendInfo
{
    public string Puuid { get; init; } = "";
    public string GameName { get; init; } = "";
    public string TagLine { get; init; } = "";
    public string Availability { get; init; } = "";
    public string Display => string.IsNullOrEmpty(TagLine) ? GameName : $"{GameName}#{TagLine}";
}
