using YummiLcu.App.Infrastructure.Atmosphere;
using YummiLcu.App.Infrastructure.Lcu;
using YummiLcu.App.Infrastructure.Pet;
using YummiLcu.App.Services;

namespace YummiLcu.App.Infrastructure.AppState;

public sealed record AppStateSnapshot(
    AppTheme CurrentTheme,
    bool IsLcuConnected,
    AppGameState CurrentGameState,
    AtmosphereState CurrentAtmosphereState,
    PetState CurrentPetState,
    bool IsRelayRunning,
    bool TestMode,
    string CurrentPage,
    bool EnablePetPlaceholder,
    bool EnableUiAnimations,
    bool EnableAtmosphereReactions,
    bool EnableToastNotifications,
    bool EnableSounds,
    double SoundVolume);
