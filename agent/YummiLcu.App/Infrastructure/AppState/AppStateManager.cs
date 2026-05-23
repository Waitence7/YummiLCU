using CommunityToolkit.Mvvm.ComponentModel;
using YummiLcu.App.Infrastructure.Atmosphere;
using YummiLcu.App.Infrastructure.Lcu;
using YummiLcu.App.Infrastructure.Pet;
using YummiLcu.App.Services;

namespace YummiLcu.App.Infrastructure.AppState;

public partial class AppStateManager : ObservableObject
{
    [ObservableProperty] private AppTheme _currentTheme = AppTheme.Cat;
    [ObservableProperty] private bool _isLcuConnected;
    [ObservableProperty] private AppGameState _currentGameState = AppGameState.Disconnected;
    [ObservableProperty] private AtmosphereState _currentAtmosphereState = AtmosphereState.Dimmed;
    [ObservableProperty] private PetState _currentPetState = PetState.Sleeping;
    [ObservableProperty] private bool _isRelayRunning;
    [ObservableProperty] private bool _testMode;
    [ObservableProperty] private string _currentPage = "Home";
    [ObservableProperty] private bool _enablePetPlaceholder = true;
    [ObservableProperty] private bool _enableUiAnimations = true;
    [ObservableProperty] private bool _enableAtmosphereReactions = true;
    [ObservableProperty] private bool _enableToastNotifications = true;
    [ObservableProperty] private bool _enableSounds;
    [ObservableProperty] private double _soundVolume = 0.5;

    public AppStateSnapshot Snapshot() =>
        new(CurrentTheme, IsLcuConnected, CurrentGameState, CurrentAtmosphereState, CurrentPetState, IsRelayRunning, TestMode, CurrentPage, EnablePetPlaceholder, EnableUiAnimations, EnableAtmosphereReactions, EnableToastNotifications, EnableSounds, SoundVolume);
}
