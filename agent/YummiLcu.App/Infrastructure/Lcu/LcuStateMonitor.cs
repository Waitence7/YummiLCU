using YummiLcu.App.Infrastructure.AppState;
using YummiLcu.App.Infrastructure.Events;
using YummiLcu.Core.Lcu;

namespace YummiLcu.App.Infrastructure.Lcu;

public sealed class LcuStateMonitor : IDisposable
{
    private readonly AppStateManager _state;
    private readonly IEventBus _events;
    private readonly ShellStateCoordinator _shellState;
    private ILcuConnector? _connector;
    private CancellationTokenSource? _pollCts;
    private AppGameState? _lastPublishedState;

    public event Action<bool>? ConnectionChanged;
    public event Action<AppGameState>? GameStateChanged;

    public LcuStateMonitor(AppStateManager state, IEventBus events, ShellStateCoordinator shellState)
    {
        _state = state;
        _events = events;
        _shellState = shellState;
    }

    public void Attach(ILcuConnector connector)
    {
        if (ReferenceEquals(_connector, connector)) return;
        Detach();
        _connector = connector;
        _connector.ConnectionChanged += OnConnectionChanged;
        OnConnectionChanged(_connector.IsConnected);
    }

    public void Detach()
    {
        StopPolling();
        if (_connector is null) return;
        _connector.ConnectionChanged -= OnConnectionChanged;
        _connector = null;
    }

    private void OnConnectionChanged(bool isConnected)
    {
        _shellState.SetLcuConnected(isConnected);
        ConnectionChanged?.Invoke(isConnected);

        if (!isConnected)
        {
            StopPolling();
            SetState(AppGameState.Disconnected);
            return;
        }

        SetState(AppGameState.Unknown);
        StartPolling();
    }

    private void StartPolling()
    {
        StopPolling();
        _pollCts = new CancellationTokenSource();
        var ct = _pollCts.Token;
        _ = Task.Run(() => PollGameflowAsync(ct), ct);
    }

    private void StopPolling()
    {
        _pollCts?.Cancel();
        _pollCts?.Dispose();
        _pollCts = null;
    }

    private async Task PollGameflowAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested && _connector is not null)
        {
            try
            {
                var phase = await _connector.GetGameflowPhaseAsync().ConfigureAwait(false);
                SetState(MapGameflowPhase(phase));
            }
            catch when (ct.IsCancellationRequested)
            {
                return;
            }
            catch
            {
                SetState(AppGameState.Unknown);
            }

            try { await Task.Delay(2000, ct).ConfigureAwait(false); }
            catch (OperationCanceledException) { return; }
        }
    }

    private static AppGameState MapGameflowPhase(string? phase) => phase switch
    {
        "None" or "Lobby" => AppGameState.Lobby,
        "Matchmaking" => AppGameState.Queue,
        "ReadyCheck" => AppGameState.MatchFound,
        "ChampSelect" => AppGameState.ChampionSelect,
        "InProgress" => AppGameState.InGame,
        "PreEndOfGame" or "EndOfGame" or "WaitingForStats" => AppGameState.EndOfGame,
        null or "" => AppGameState.Unknown,
        _ => AppGameState.Unknown,
    };

    public void SetDebugGameState(AppGameState state) => SetState(state);

    private void SetState(AppGameState state)
    {
        _state.CurrentGameState = state;
        if (_lastPublishedState == state) return;
        _lastPublishedState = state;
        _events.Publish(new AppGameStateChangedEvent(state));
        GameStateChanged?.Invoke(state);
    }

    public void Dispose() => Detach();
}
