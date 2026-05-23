using YummiLcu.App.Contracts.Audio;
using YummiLcu.App.Infrastructure.Animation;
using YummiLcu.App.Infrastructure.Audio;
using YummiLcu.App.Infrastructure.Atmosphere;
using YummiLcu.App.Infrastructure.AppState;
using YummiLcu.App.Infrastructure.Events;
using YummiLcu.App.Infrastructure.Lcu;
using YummiLcu.App.Infrastructure.Pet;
using YummiLcu.App.Infrastructure.Settings;
using YummiLcu.App.Infrastructure.Theme;
using YummiLcu.App.Infrastructure.Toasts;

namespace YummiLcu.App.Composition;

public sealed class AppServices
{
    public AppServices()
    {
        State = new AppStateManager();
        Events = new EventBus();
        Preferences = new InteractionPreferencesService(State);
        ShellState = new ShellStateCoordinator(State, Events);
        LcuStates = new LcuStateMonitor(State, Events, ShellState);
        Atmosphere = new AtmosphereController(State, Events);
        Atmosphere.Start();
        Pets = new PetController(State, Events);
        Pets.Start();
        Themes = new ThemeManager(State, Events);
        Animations = new AnimationManager(State);
        Toasts = new ToastManager(State, Events);
        Toasts.Start();
        var audio = new AudioManager(Events);
        audio.SetMuted(!Preferences.Current.EnableSounds);
        audio.SetVolume(Preferences.Current.SoundVolume);
        audio.Start();
        Audio = audio;
    }

    public AppStateManager State { get; }
    public IEventBus Events { get; }
    public InteractionPreferencesService Preferences { get; }
    public ShellStateCoordinator ShellState { get; }
    public LcuStateMonitor LcuStates { get; }
    public AtmosphereController Atmosphere { get; }
    public PetController Pets { get; }
    public IThemeManager Themes { get; }
    public IAnimationManager Animations { get; }
    public ToastManager Toasts { get; }
    public IAudioSystem Audio { get; }
}
