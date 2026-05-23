using YummiLcu.App.Infrastructure.AppState;
using YummiLcu.App.Infrastructure.Events;
using YummiLcu.App.Services;

namespace YummiLcu.App.Infrastructure.Theme;

public sealed class ThemeManager : IThemeManager
{
    private readonly AppStateManager _state;
    private readonly IEventBus _events;

    public ThemeManager(AppStateManager state, IEventBus events)
    {
        _state = state;
        _events = events;
    }

    public AppTheme Current => ThemeService.Current;

    public void Apply(AppTheme theme)
    {
        ThemeService.Apply(theme);
        _state.CurrentTheme = theme;
        _events.Publish(new ThemeChangedEvent(theme));
    }
}
