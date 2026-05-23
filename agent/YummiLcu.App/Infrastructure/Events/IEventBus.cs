namespace YummiLcu.App.Infrastructure.Events;

public interface IEventBus
{
    IDisposable Subscribe<TEvent>(Action<TEvent> handler);
    void Publish<TEvent>(TEvent appEvent);
}
