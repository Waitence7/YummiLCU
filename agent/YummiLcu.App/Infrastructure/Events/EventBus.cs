namespace YummiLcu.App.Infrastructure.Events;

public sealed class EventBus : IEventBus
{
    private readonly object _gate = new();
    private readonly Dictionary<Type, List<Delegate>> _handlers = new();

    public IDisposable Subscribe<TEvent>(Action<TEvent> handler)
    {
        lock (_gate)
        {
            var eventType = typeof(TEvent);
            if (!_handlers.TryGetValue(eventType, out var handlers))
            {
                handlers = new List<Delegate>();
                _handlers[eventType] = handlers;
            }

            handlers.Add(handler);
        }

        return new Subscription(() => Unsubscribe(handler));
    }

    public void Publish<TEvent>(TEvent appEvent)
    {
        List<Delegate> handlers;
        lock (_gate)
        {
            if (!_handlers.TryGetValue(typeof(TEvent), out var registered))
                return;

            handlers = registered.ToList();
        }

        foreach (var handler in handlers.OfType<Action<TEvent>>())
            handler(appEvent);
    }

    private void Unsubscribe<TEvent>(Action<TEvent> handler)
    {
        lock (_gate)
        {
            if (!_handlers.TryGetValue(typeof(TEvent), out var handlers))
                return;

            handlers.Remove(handler);
            if (handlers.Count == 0)
                _handlers.Remove(typeof(TEvent));
        }
    }

    private sealed class Subscription : IDisposable
    {
        private readonly Action _dispose;
        private bool _disposed;

        public Subscription(Action dispose) => _dispose = dispose;

        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;
            _dispose();
        }
    }
}
