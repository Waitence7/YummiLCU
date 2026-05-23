using YummiLcu.App.Infrastructure.AppState;
using YummiLcu.App.Infrastructure.Events;
using YummiLcu.App.Infrastructure.Lcu;

namespace YummiLcu.App.Infrastructure.Atmosphere;

public sealed class AtmosphereController
{
    private readonly AppStateManager _state;
    private readonly IEventBus _events;
    private IDisposable? _gameStateSubscription;
    private AtmosphereState? _lastPublishedState;

    public AtmosphereController(AppStateManager state, IEventBus events)
    {
        _state = state;
        _events = events;
    }

    public void Start()
    {
        if (_gameStateSubscription is not null) return;
        _gameStateSubscription = _events.Subscribe<AppGameStateChangedEvent>(OnGameStateChanged);
        SetState(MapGameState(_state.CurrentGameState));
    }

    public void Stop()
    {
        _gameStateSubscription?.Dispose();
        _gameStateSubscription = null;
    }

    private void OnGameStateChanged(AppGameStateChangedEvent appEvent)
    {
        if (!_state.EnableAtmosphereReactions) return;
        SetState(MapGameState(appEvent.State));
    }

    private static AtmosphereState MapGameState(AppGameState state) => state switch
    {
        AppGameState.Disconnected => AtmosphereState.Dimmed,
        AppGameState.Lobby => AtmosphereState.Calm,
        AppGameState.Queue => AtmosphereState.Active,
        AppGameState.MatchFound => AtmosphereState.Alert,
        AppGameState.ChampionSelect => AtmosphereState.Focused,
        AppGameState.InGame => AtmosphereState.Resting,
        AppGameState.EndOfGame => AtmosphereState.Result,
        _ => AtmosphereState.Neutral,
    };

    private void SetState(AtmosphereState state)
    {
        _state.CurrentAtmosphereState = state;
        if (_lastPublishedState == state) return;
        _lastPublishedState = state;
        _events.Publish(new AtmosphereStateChangedEvent(state));
    }
}
