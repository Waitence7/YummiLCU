using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using YummiLcu.Core.Lcu;
using YummiLcu.Core.Lcu.Models;

namespace YummiLcu.App.ViewModels;

public partial class LobbyViewModel : ObservableObject
{
    private readonly ILcuConnector _lcu;

    [ObservableProperty] private bool _isInLobby;
    [ObservableProperty] private string _queueLabel = "";
    [ObservableProperty] private string _memberText = "";
    [ObservableProperty] private int _slotCount = 5;
    [ObservableProperty] private int _filledSlots = 0;
    [ObservableProperty] private bool _isSearching;
    [ObservableProperty] private string _etaText = "";
    [ObservableProperty] private string _elapsedText = "";
    [ObservableProperty] private string _playButtonText = "게임 찾기";

    public ObservableCollection<FriendInfo> Friends { get; } = new();

    public LobbyViewModel(ILcuConnector lcu)
    {
        _lcu = lcu;
        _lcu.LobbyChanged += ApplyLobby;
        _lcu.MatchmakingChanged += ApplyMatchmaking;
    }

    private void ApplyLobby(LobbyInfo lobby)
    {
        IsInLobby = lobby.IsInLobby;
        QueueLabel = lobby.IsInLobby ? lobby.QueueLabel : "로비 없음";
        MemberText = lobby.IsInLobby ? $"{lobby.MemberCount} / {lobby.MaxMembers}" : "";
        SlotCount = lobby.IsInLobby ? lobby.MaxMembers : 5;
        FilledSlots = lobby.IsInLobby ? lobby.MemberCount : 0;
        if (!IsSearching)
            PlayButtonText = IsInLobby ? "게임 찾기" : "로비를 만드세요";
    }

    private void ApplyMatchmaking(MatchmakingStatus status)
    {
        IsSearching = status.IsSearching;
        if (status.IsSearching)
        {
            PlayButtonText = "찾는 중…";
            EtaText = $"예상 대기  {MatchmakingStatus.FormatDuration(status.EstimatedQueueTimeSeconds)}";
            ElapsedText = $"경과  {MatchmakingStatus.FormatDuration(status.TimeInQueueSeconds)}";
        }
        else
        {
            EtaText = "";
            ElapsedText = "";
            PlayButtonText = IsInLobby ? "게임 찾기" : "로비를 만드세요";
        }
    }

    [RelayCommand]
    public async Task RefreshFriendsAsync()
    {
        Friends.Clear();
        foreach (var f in await _lcu.GetFriendsAsync())
            Friends.Add(f);
    }

    [RelayCommand]
    public Task CreateRankedLobbyAsync() => _lcu.RunActionAsync("create_ranked_lobby");

    [RelayCommand]
    public Task CreateNormalLobbyAsync() => _lcu.RunActionAsync("create_normal_lobby");

    [RelayCommand]
    public Task LeaveLobbyAsync() => _lcu.RunActionAsync("leave_lobby");

    [RelayCommand]
    public async Task PlayOrCancelAsync()
    {
        if (IsSearching)
            await _lcu.RunActionAsync("queue_cancel");
        else if (IsInLobby)
            await _lcu.RunActionAsync("queue_start");
    }

    [RelayCommand]
    public async Task LoadedAsync()
    {
        ApplyLobby(await _lcu.GetLobbyAsync());
        ApplyMatchmaking(await _lcu.GetMatchmakingAsync());
        await RefreshFriendsAsync();
    }
}
