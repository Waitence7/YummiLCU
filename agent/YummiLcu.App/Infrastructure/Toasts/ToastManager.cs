using YummiLcu.App.Infrastructure.AppState;
using YummiLcu.App.Infrastructure.Events;
using YummiLcu.App.Infrastructure.Lcu;

namespace YummiLcu.App.Infrastructure.Toasts;

public sealed class ToastManager
{
    private static readonly TimeSpan DuplicateWindow = TimeSpan.FromSeconds(2.5);
    private readonly AppStateManager _state;
    private readonly IEventBus _events;
    private readonly Dictionary<string, DateTimeOffset> _lastPublished = new();
    private IDisposable? _lcuConnectionSubscription;
    private IDisposable? _gameStateSubscription;

    public ToastManager(AppStateManager state, IEventBus events)
    {
        _state = state;
        _events = events;
    }

    public void Start()
    {
        if (_lcuConnectionSubscription is not null) return;
        _lcuConnectionSubscription = _events.Subscribe<LcuConnectionChangedEvent>(OnLcuConnectionChanged);
        _gameStateSubscription = _events.Subscribe<AppGameStateChangedEvent>(OnGameStateChanged);
    }

    public void Stop()
    {
        _lcuConnectionSubscription?.Dispose();
        _gameStateSubscription?.Dispose();
        _lcuConnectionSubscription = null;
        _gameStateSubscription = null;
    }

    public void Request(ToastType type, string title, string message, string? deduplicationKey = null)
    {
        if (!_state.EnableToastNotifications) return;

        var key = deduplicationKey ?? $"{type}:{title}:{message}";
        var now = DateTimeOffset.Now;
        if (_lastPublished.TryGetValue(key, out var last) && now - last < DuplicateWindow)
            return;

        _lastPublished[key] = now;
        _events.Publish(new ToastRequestedEvent(type, title, message, key));
    }

    private void OnLcuConnectionChanged(LcuConnectionChangedEvent appEvent)
    {
        if (appEvent.IsConnected)
            Request(ToastType.Success, "LCU connected", "League Client connection is ready.", "lcu-connected");
        else
            Request(ToastType.Warning, "LCU disconnected", "Waiting for the League Client.", "lcu-disconnected");
    }

    private void OnGameStateChanged(AppGameStateChangedEvent appEvent)
    {
        if (appEvent.State == AppGameState.MatchFound)
            Request(ToastType.Success, "Match found", "Ready check is available.", "match-found");
    }
}
