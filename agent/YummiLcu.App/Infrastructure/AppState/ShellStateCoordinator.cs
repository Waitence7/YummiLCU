using YummiLcu.App.Infrastructure.Events;

namespace YummiLcu.App.Infrastructure.AppState;

public sealed class ShellStateCoordinator
{
    private readonly AppStateManager _state;
    private readonly IEventBus _events;
    private bool? _lastPublishedTestMode;
    private bool? _lastPublishedLcuConnected;
    private bool? _lastPublishedRelayRunning;
    private string? _lastPublishedPage;

    public ShellStateCoordinator(AppStateManager state, IEventBus events)
    {
        _state = state;
        _events = events;
    }

    public void SetTestMode(bool isEnabled)
    {
        _state.TestMode = isEnabled;
        if (_lastPublishedTestMode == isEnabled) return;
        _lastPublishedTestMode = isEnabled;
        _events.Publish(new TestModeChangedEvent(isEnabled));
    }

    public void SetLcuConnected(bool isConnected)
    {
        _state.IsLcuConnected = isConnected;
        if (_lastPublishedLcuConnected == isConnected) return;
        _lastPublishedLcuConnected = isConnected;
        _events.Publish(new LcuConnectionChangedEvent(isConnected));
    }

    public void SetRelayRunning(bool isRunning)
    {
        _state.IsRelayRunning = isRunning;
        if (_lastPublishedRelayRunning == isRunning) return;
        _lastPublishedRelayRunning = isRunning;
        _events.Publish(new RelayStateChangedEvent(isRunning));
    }

    public void SetCurrentPage(string pageName)
    {
        _state.CurrentPage = pageName;
        if (_lastPublishedPage == pageName) return;
        _lastPublishedPage = pageName;
        _events.Publish(new NavigationChangedEvent(pageName));
    }
}
