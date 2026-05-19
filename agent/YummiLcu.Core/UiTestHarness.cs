using YummiLcu.Core.Lcu;
using YummiLcu.Core.Lcu.Models;

namespace YummiLcu.Core;

public sealed class UiTestHarness : IDisposable
{
    private readonly System.Timers.Timer _queueTimer = new(1000);
    private LobbyInfo _lobby = LobbyInfo.None;
    private MatchmakingStatus _matchmaking = MatchmakingStatus.Idle;
    private double _estimatedSeconds = 95;

    public event Action<LobbyInfo>? LobbyChanged;
    public event Action<MatchmakingStatus>? MatchmakingChanged;
    public event Action<string>? Log;

    public bool IsActive { get; private set; }
    public LobbyInfo CurrentLobby => _lobby;
    public MatchmakingStatus CurrentMatchmaking => _matchmaking;

    public UiTestHarness()
    {
        _queueTimer.Elapsed += (_, _) => TickQueue();
    }

    public void Start()
    {
        if (IsActive) return;
        IsActive = true;
        _lobby = LobbyInfo.None;
        _matchmaking = MatchmakingStatus.Idle;
        PublishLobby();
        PublishMatchmaking();
        Log?.Invoke("테스트 모드 — 롤 클라이언트·Relay 없이 UI 동작");
    }

    public void Stop()
    {
        if (!IsActive) return;
        IsActive = false;
        _queueTimer.Stop();
        _lobby = LobbyInfo.None;
        _matchmaking = MatchmakingStatus.Idle;
        PublishLobby();
        PublishMatchmaking();
        Log?.Invoke("테스트 모드 종료");
    }

    public Task<(bool Ok, string Message)> RunActionAsync(string action)
    {
        if (!IsActive)
            return Task.FromResult((false, "테스트 모드가 꺼져 있음"));

        var (ok, msg) = action switch
        {
            "create_ranked_lobby" => CreateLobby(LcuQueue.RankedSolo),
            "create_normal_lobby" => CreateLobby(LcuQueue.NormalDraft),
            "leave_lobby" => LeaveLobby(),
            "queue_start" => StartQueue(),
            "queue_cancel" => CancelQueue(),
            "play_ranked_solo" => CreateAndQueue(LcuQueue.RankedSolo),
            "play_normal_draft" => CreateAndQueue(LcuQueue.NormalDraft),
            "set_status" or "reset_status" => Mock("상메 (시뮬)"),
            "accept_match" => Mock("매치 수락 (시뮬)"),
            "decline_match" => Mock("매치 거절 (시뮬)"),
            "party_ready" => Mock("파티 준비 (시뮬)"),
            "champ_reroll" => Mock("리롤 (시뮬)"),
            "dodge" => Dodge(),
            "reconnect" => Mock("재접속 (시뮬)"),
            "claim_all_rewards" => Mock("보상 (시뮬)"),
            "launch_client" => (false, "테스트 모드 — 실행 생략"),
            "quit_client" => (false, "테스트 모드 — 종료 생략"),
            "ping" => (true, "pong (시뮬)"),
            _ => (false, $"시뮬 미지원: {action}"),
        };
        return Task.FromResult((ok, msg));
    }

    private (bool Ok, string Message) CreateLobby(int queueId)
    {
        CancelQueueInternal();
        _lobby = new LobbyInfo(true, queueId, LobbyInfo.LabelForQueue(queueId), 1, 5);
        PublishLobby();
        Log?.Invoke($"{_lobby.QueueLabel} 로비 (시뮬)");
        return (true, "ok");
    }

    private (bool Ok, string Message) LeaveLobby()
    {
        CancelQueueInternal();
        _lobby = LobbyInfo.None;
        PublishLobby();
        return (true, "ok");
    }

    private (bool Ok, string Message) CreateAndQueue(int queueId)
    {
        var r = CreateLobby(queueId);
        return r.Ok ? StartQueue() : r;
    }

    private (bool Ok, string Message) StartQueue()
    {
        if (!_lobby.IsInLobby) return (false, "로비 없음");
        _estimatedSeconds = _lobby.QueueId == LcuQueue.RankedSolo ? 120 : 75;
        _matchmaking = new MatchmakingStatus(true, 0, _estimatedSeconds);
        _queueTimer.Start();
        PublishMatchmaking();
        return (true, "매칭 시작 (시뮬)");
    }

    private (bool Ok, string Message) CancelQueue()
    {
        if (!_matchmaking.IsSearching) return (false, "매칭 중 아님");
        CancelQueueInternal();
        return (true, "ok");
    }

    private (bool Ok, string Message) Dodge()
    {
        CancelQueueInternal();
        return (true, "닷지 (시뮬)");
    }

    private static (bool Ok, string Message) Mock(string msg) => (true, msg);

    private void CancelQueueInternal()
    {
        _queueTimer.Stop();
        _matchmaking = MatchmakingStatus.Idle;
        PublishMatchmaking();
    }

    private void TickQueue()
    {
        if (!_matchmaking.IsSearching) return;
        _matchmaking = new MatchmakingStatus(true, _matchmaking.TimeInQueueSeconds + 1, _estimatedSeconds);
        PublishMatchmaking();
    }

    private void PublishLobby() => LobbyChanged?.Invoke(_lobby);
    private void PublishMatchmaking() => MatchmakingChanged?.Invoke(_matchmaking);

    public void Dispose() => _queueTimer.Dispose();
}
