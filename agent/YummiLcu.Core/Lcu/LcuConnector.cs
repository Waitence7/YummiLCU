using System.Text.Json;
using YummiLcu.Core.Lcu.Models;

namespace YummiLcu.Core.Lcu;

public sealed class LcuConnector : ILcuConnector, IDisposable
{
    private readonly UiTestHarness _testHarness = new();
    private AgentConfig _config = new();
    private LcuClient? _lcu;
    private CancellationTokenSource? _watchCts;
    private LobbyInfo _lastLobby = LobbyInfo.None;
    private bool _testMode;

    public bool IsConnected => _testMode ? _testHarness.IsActive : _lcu is not null;
    public bool IsTestMode => _testMode;

    public event Action<bool>? TestModeChanged;
    public event Action<bool>? ConnectionChanged;
    public event Action<LobbyInfo>? LobbyChanged;
    public event Action<MatchmakingStatus>? MatchmakingChanged;
    public event Action<string>? Log;

    public LcuConnector()
    {
        _testHarness.LobbyChanged += l =>
        {
            _lastLobby = l;
            LobbyChanged?.Invoke(l);
        };
        _testHarness.MatchmakingChanged += m => MatchmakingChanged?.Invoke(m);
        _testHarness.Log += s => Log?.Invoke(s);
    }

    public void SetTestMode(bool enabled)
    {
        if (_testMode == enabled) return;
        _testMode = enabled;
        Disconnect();
        if (enabled)
        {
            _testHarness.Start();
            ConnectionChanged?.Invoke(true);
            Log?.Invoke("테스트 모드 ON — 롤 클라이언트·lockfile 없이 UI 확인 가능");
        }
        else
        {
            Log?.Invoke("테스트 모드 OFF — 실제 LCU 연결이 필요합니다");
        }
        TestModeChanged?.Invoke(enabled);
    }

    private static readonly ChampSelectSessionInfo MockChampSelect = new()
    {
        IsActive = true,
        Phase = "PICK (시뮬)",
        LocalPlayerCellId = 0,
        Actions =
        [
            new ChampSelectAction
            {
                Id = 101,
                Type = "pick",
                ChampionId = 0,
                Completed = false,
                IsInProgress = true,
                IsAllyAction = true,
            },
        ],
    };

    private static readonly IReadOnlyList<PerkPageInfo> MockPerkPages =
    [
        new PerkPageInfo { Id = 9001, Name = "테스트 룬 — 정밀" },
        new PerkPageInfo { Id = 9002, Name = "테스트 룬 — 마법" },
    ];

    public async Task<bool> ConnectAsync(AgentConfig config, CancellationToken ct = default)
    {
        _config = config;
        if (_testMode)
        {
            _testHarness.Start();
            ConnectionChanged?.Invoke(true);
            return true;
        }

        var path = config.ResolveLockfilePath();
        if (path != null && !File.Exists(path)) path = null;
        path ??= LcuClient.FindLockfilePath();
        if (path is null)
        {
            Log?.Invoke("lockfile 없음");
            return false;
        }

        var deadline = DateTime.UtcNow.AddMinutes(2);
        while (DateTime.UtcNow < deadline && !ct.IsCancellationRequested)
        {
            var (client, error) = LcuClient.TryFromLockfile(path);
            if (client is not null)
            {
                _lcu = client;
                Log?.Invoke($"LCU 연결: {path}");
                StartWatchLoops();
                ConnectionChanged?.Invoke(true);
                return true;
            }
            Log?.Invoke($"LCU 대기: {error}");
            await Task.Delay(2000, ct);
        }
        return false;
    }

    public void Disconnect()
    {
        _watchCts?.Cancel();
        _watchCts?.Dispose();
        _watchCts = null;
        _lcu?.Dispose();
        _lcu = null;
        if (_testHarness.IsActive) _testHarness.Stop();
        _lastLobby = LobbyInfo.None;
        ConnectionChanged?.Invoke(false);
        MatchmakingChanged?.Invoke(MatchmakingStatus.Idle);
        LobbyChanged?.Invoke(LobbyInfo.None);
    }

    private void StartWatchLoops()
    {
        _watchCts = new CancellationTokenSource();
        var ct = _watchCts.Token;
        _ = Task.Run(() => LobbyWatchAsync(ct), ct);
        _ = Task.Run(() => MatchmakingWatchAsync(ct), ct);
    }

    private async Task LobbyWatchAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested && _lcu is not null)
        {
            var lobby = await _lcu.GetLobbyAsync();
            if (lobby != _lastLobby)
            {
                _lastLobby = lobby;
                LobbyChanged?.Invoke(lobby);
            }
            await Task.Delay(1500, ct);
        }
    }

    private async Task MatchmakingWatchAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested && _lcu is not null)
        {
            var status = await _lcu.GetMatchmakingStatusAsync();
            MatchmakingChanged?.Invoke(status);
            await Task.Delay(status.IsSearching ? 1000 : 2500, ct);
        }
    }

    public async Task<SummonerInfo?> GetCurrentSummonerAsync()
    {
        if (_testMode)
            return new SummonerInfo { DisplayName = "테스트 소환사", Level = 999, ProfileIconId = 29 };
        if (_lcu is null) return null;
        return await _lcu.GetCurrentSummonerAsync();
    }

    public async Task<byte[]?> GetProfileIconBytesAsync(int iconId)
    {
        if (_testMode || _lcu is null) return null;
        return await _lcu.GetBytesAsync(_lcu.ProfileIconAssetPath(iconId));
    }

    public Task<ChatMeInfo?> GetChatMeAsync() =>
        _testMode ? Task.FromResult<ChatMeInfo?>(new ChatMeInfo
        {
            StatusMessage = StatusMessageHelper.DefaultYummiClient,
            Availability = "online",
        }) : _lcu is null ? Task.FromResult<ChatMeInfo?>(null) : _lcu.GetChatMeAsync();

    public async Task<bool> SetStatusMessageAsync(string text)
    {
        if (_testMode) return true;
        if (_lcu is null) return false;
        var normalized = StatusMessageHelper.Normalize(text);
        if (!StatusMessageHelper.TryValidate(normalized, out _)) return false;
        return await _lcu.SetStatusMessageAsync(normalized);
    }

    public Task<LobbyInfo> GetLobbyAsync() =>
        _testMode ? Task.FromResult(_lastLobby) : _lcu is null ? Task.FromResult(LobbyInfo.None) : _lcu.GetLobbyAsync();

    public Task<MatchmakingStatus> GetMatchmakingAsync() =>
        _testMode ? Task.FromResult(_testHarness.CurrentMatchmaking) :
        _lcu is null ? Task.FromResult(MatchmakingStatus.Idle) : _lcu.GetMatchmakingStatusAsync();

    public Task<IReadOnlyList<FriendInfo>> GetFriendsAsync() =>
        _testMode ? Task.FromResult<IReadOnlyList<FriendInfo>>(new[]
        {
            new FriendInfo { GameName = "친구1", TagLine = "KR1", Availability = "online" },
            new FriendInfo { GameName = "친구2", TagLine = "KR2", Availability = "away" },
        }) : _lcu is null ? Task.FromResult<IReadOnlyList<FriendInfo>>(Array.Empty<FriendInfo>()) : _lcu.GetFriendsAsync();

    public Task<ChampSelectSessionInfo?> GetChampSelectSessionAsync() =>
        _testMode ? Task.FromResult<ChampSelectSessionInfo?>(MockChampSelect) :
        _lcu is null ? Task.FromResult<ChampSelectSessionInfo?>(null) : _lcu.GetChampSelectSessionAsync();

    public Task<IReadOnlyList<PerkPageInfo>> GetPerkPagesAsync() =>
        _testMode ? Task.FromResult(MockPerkPages) :
        _lcu is null ? Task.FromResult<IReadOnlyList<PerkPageInfo>>(Array.Empty<PerkPageInfo>()) : _lcu.GetPerkPagesAsync();

    public async Task<(bool Ok, string Message)> RunActionAsync(string action, string? payloadText = null, CancellationToken ct = default)
    {
        if (_testMode)
            return await _testHarness.RunActionAsync(action);

        if (!AllowedActions.IsAllowed(action))
            return (false, "unknown action");

        if (action == "launch_client")
            return LeagueLauncher.TryLaunch();

        if (_lcu is null)
            return (false, "LCU 미연결");

        JsonDocument? payloadDoc = null;
        JsonElement? payload = null;
        if (!string.IsNullOrWhiteSpace(payloadText))
        {
            payloadDoc = JsonDocument.Parse(JsonSerializer.Serialize(new { text = payloadText }));
            payload = payloadDoc.RootElement.Clone();
        }
        try
        {
            return await AllowedActions.ExecuteAsync(action, new ActionContext(_lcu, _config, payload));
        }
        finally
        {
            payloadDoc?.Dispose();
        }
    }

    public async Task<bool> PickChampionAsync(int actionId, int championId)
    {
        if (_testMode)
        {
            Log?.Invoke($"픽 시뮬: action={actionId}, champion={championId}");
            return true;
        }
        return _lcu is not null && await _lcu.PatchChampSelectActionAsync(actionId, championId);
    }

    public async Task<bool> ApplyPerkPageAsync(long pageId)
    {
        if (_testMode)
        {
            Log?.Invoke($"룬 적용 시뮬: pageId={pageId}");
            return true;
        }
        return _lcu is not null && await _lcu.SetCurrentPerkPageAsync(pageId);
    }

    public void Dispose()
    {
        Disconnect();
        _testHarness.Dispose();
    }
}
