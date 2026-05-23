using YummiLcu.App.Infrastructure.Atmosphere;
using YummiLcu.App.Infrastructure.Lcu;
using YummiLcu.App.Infrastructure.Pet;
using YummiLcu.App.Infrastructure.Toasts;
using YummiLcu.App.Services;

namespace YummiLcu.App.Infrastructure.Events;

public sealed record ThemeChangedEvent(AppTheme Theme);
public sealed record LcuConnectionChangedEvent(bool IsConnected);
public sealed record AppGameStateChangedEvent(AppGameState State);
public sealed record AtmosphereStateChangedEvent(AtmosphereState State);
public sealed record PetStateChangedEvent(PetState State);
public sealed record RelayStateChangedEvent(bool IsRunning);
public sealed record TestModeChangedEvent(bool IsEnabled);
public sealed record NavigationChangedEvent(string PageName);
public sealed record ToastRequestedEvent(ToastType Type, string Title, string Message, string? DeduplicationKey = null);
