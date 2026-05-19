namespace YummiLcu.Agent;

/// <summary>LCU/Relay 없이 로비·매칭 UI만 검증하는 로컬 시뮬레이터.</summary>
internal sealed class UiTestHarness : IDisposable
{
    private readonly System.Windows.Forms.Timer _queueTimer = new() { Interval = 1000 };
    private LobbyInfo _lobby = LobbyInfo.None;
    private MatchmakingStatus _matchmaking = MatchmakingStatus.Idle;
    private double _estimatedSeconds = 95;

    public event Action<LobbyInfo>? LobbyChanged;
    public event Action<MatchmakingStatus>? MatchmakingChanged;
    public event Action<string>? Log;

    public bool IsActive { get; private set; }

    public UiTestHarness()
    {
        _queueTimer.Tick += (_, _) => TickQueue();
    }

    public void Start()
    {
        if (IsActive)
            return;
        IsActive = true;
        _lobby = LobbyInfo.None;
        _matchmaking = MatchmakingStatus.Idle;
        PublishLobby();
        PublishMatchmaking();
        Log?.Invoke("테스트 모드 — 롤 클라이언트·Relay 없이 UI 동작");
    }

    public void Stop()
    {
        if (!IsActive)
            return;
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
            "set_status" => Mock("상메 적용 (시뮬)"),
            "reset_status" => Mock("기본 상메 (시뮬)"),
            "accept_match" => Mock("매치 수락 (시뮬)"),
            "decline_match" => Mock("매치 거절 (시뮬)"),
            "party_ready" => Mock("파티 준비 (시뮬)"),
            "champ_reroll" => Mock("챔프 리롤 (시뮬)"),
            "dodge" => Dodge(),
            "reconnect" => Mock("재접속 (시뮬)"),
            "claim_all_rewards" => Mock("보상 수령 (시뮬)"),
            "launch_client" => (false, "테스트 모드 — 클라이언트 실행 생략"),
            "quit_client" => (false, "테스트 모드 — 클라이언트 종료 생략"),
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
        var msg = $"{_lobby.QueueLabel} 로비 (시뮬)";
        Log?.Invoke(msg);
        return (true, msg);
    }

    private (bool Ok, string Message) LeaveLobby()
    {
        CancelQueueInternal();
        _lobby = LobbyInfo.None;
        PublishLobby();
        Log?.Invoke("로비 나가기 (시뮬)");
        return (true, "ok");
    }

    private (bool Ok, string Message) CreateAndQueue(int queueId)
    {
        var (ok, msg) = CreateLobby(queueId);
        if (!ok)
            return (ok, msg);
        return StartQueue();
    }

    private (bool Ok, string Message) StartQueue()
    {
        if (!_lobby.IsInLobby)
            return (false, "로비 없음 — 먼저 로비를 만드세요");

        _estimatedSeconds = _lobby.QueueId == LcuQueue.RankedSolo ? 120 : 75;
        _matchmaking = new MatchmakingStatus(true, 0, _estimatedSeconds);
        _queueTimer.Start();
        PublishMatchmaking();
        Log?.Invoke("매칭 시작 (시뮬)");
        return (true, "매칭 시작 (시뮬)");
    }

    private (bool Ok, string Message) CancelQueue()
    {
        if (!_matchmaking.IsSearching)
            return (false, "매칭 중 아님");
        CancelQueueInternal();
        Log?.Invoke("매칭 취소 (시뮬)");
        return (true, "ok");
    }

    private (bool Ok, string Message) Dodge()
    {
        CancelQueueInternal();
        Log?.Invoke("닷지 (시뮬)");
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
        if (!_matchmaking.IsSearching)
            return;

        var elapsed = _matchmaking.TimeInQueueSeconds + 1;
        _matchmaking = new MatchmakingStatus(true, elapsed, _estimatedSeconds);
        PublishMatchmaking();
    }

    private void PublishLobby() => LobbyChanged?.Invoke(_lobby);
    private void PublishMatchmaking() => MatchmakingChanged?.Invoke(_matchmaking);

    public void Dispose()
    {
        _queueTimer.Stop();
        _queueTimer.Dispose();
    }
}
