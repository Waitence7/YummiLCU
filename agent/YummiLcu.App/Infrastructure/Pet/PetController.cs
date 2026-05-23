using YummiLcu.App.Contracts.Pet;
using YummiLcu.App.Infrastructure.AppState;
using YummiLcu.App.Infrastructure.Events;
using YummiLcu.App.Infrastructure.Lcu;

namespace YummiLcu.App.Infrastructure.Pet;

public sealed class PetController : IPetSystem
{
    private readonly AppStateManager _state;
    private readonly IEventBus _events;
    private IDisposable? _gameStateSubscription;
    private PetState? _lastPublishedState;

    public PetController(AppStateManager state, IEventBus events)
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

    private void OnGameStateChanged(AppGameStateChangedEvent appEvent) =>
        SetState(MapGameState(appEvent.State));

    private static PetState MapGameState(AppGameState state) => state switch
    {
        AppGameState.Lobby => PetState.Idle,
        AppGameState.Queue => PetState.Waiting,
        AppGameState.MatchFound => PetState.Excited,
        AppGameState.ChampionSelect => PetState.Focused,
        AppGameState.InGame => PetState.Hidden,
        AppGameState.EndOfGame => PetState.Curious,
        AppGameState.Disconnected => PetState.Sleeping,
        _ => PetState.Idle,
    };

    private void SetState(PetState state)
    {
        _state.CurrentPetState = state;
        if (_lastPublishedState == state) return;
        _lastPublishedState = state;
        _events.Publish(new PetStateChangedEvent(state));
    }
}
