using YummiLcu.Core.Lcu.Models;

namespace YummiLcu.Core.Lcu;

public interface ILcuConnector
{
    bool IsConnected { get; }
    bool IsTestMode { get; }

    event Action<bool>? TestModeChanged;
    event Action<bool>? ConnectionChanged;
    event Action<LobbyInfo>? LobbyChanged;
    event Action<MatchmakingStatus>? MatchmakingChanged;
    event Action<string>? Log;

    void SetTestMode(bool enabled);
    Task<bool> ConnectAsync(AgentConfig config, CancellationToken ct = default);
    void Disconnect();

    Task<SummonerInfo?> GetCurrentSummonerAsync();
    Task<byte[]?> GetProfileIconBytesAsync(int iconId);
    Task<ChatMeInfo?> GetChatMeAsync();
    Task<bool> SetStatusMessageAsync(string text);
    Task<LobbyInfo> GetLobbyAsync();
    Task<MatchmakingStatus> GetMatchmakingAsync();
    Task<string?> GetGameflowPhaseAsync();
    Task<IReadOnlyList<FriendInfo>> GetFriendsAsync();
    Task<ChampSelectSessionInfo?> GetChampSelectSessionAsync();
    Task<IReadOnlyList<PerkPageInfo>> GetPerkPagesAsync();
    Task<(bool Ok, string Message)> RunActionAsync(string action, string? payloadText = null, CancellationToken ct = default);
    Task<bool> PickChampionAsync(int actionId, int championId);
    Task<bool> ApplyPerkPageAsync(long pageId);
}
