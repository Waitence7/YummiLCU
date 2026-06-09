using System.Text.Json.Serialization;

namespace YummiLcu.Core.Lcu.Models;

public sealed class GuildMatchLcuParticipant
{
    [JsonPropertyName("gameName")]
    public string GameName { get; init; } = "";

    [JsonPropertyName("tagLine")]
    public string TagLine { get; init; } = "";

    [JsonPropertyName("teamId")]
    public int? TeamId { get; init; }

    [JsonPropertyName("won")]
    public bool? Won { get; init; }
}

public sealed class GuildMatchLcuPayload
{
    [JsonPropertyName("source")]
    public string Source { get; init; } = "lcu-agent";

    [JsonPropertyName("gameflowPhase")]
    public string GameflowPhase { get; init; } = "";

    [JsonPropertyName("capturedAt")]
    public string CapturedAt { get; init; } = "";

    [JsonPropertyName("participants")]
    public IReadOnlyList<GuildMatchLcuParticipant> Participants { get; init; } =
        Array.Empty<GuildMatchLcuParticipant>();

    [JsonPropertyName("gameResult")]
    public GuildMatchLcuGameResult? GameResult { get; init; }

    [JsonPropertyName("eogStats")]
    public object? EogStats { get; init; }
}

public sealed class GuildMatchLcuGameResult
{
    [JsonPropertyName("didWin")]
    public bool? DidWin { get; init; }

    [JsonPropertyName("winnerTeamSide")]
    public string? WinnerTeamSide { get; init; }
}
