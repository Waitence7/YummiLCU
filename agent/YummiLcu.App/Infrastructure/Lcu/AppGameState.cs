namespace YummiLcu.App.Infrastructure.Lcu;

public enum AppGameState
{
    Unknown,
    Disconnected,
    Lobby,
    Queue,
    MatchFound,
    ChampionSelect,
    InGame,
    EndOfGame,
}
