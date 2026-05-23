using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using YummiLcu.App.Infrastructure.Lcu;
using YummiLcu.App.Composition;
using YummiLcu.App.Infrastructure.Settings;

namespace YummiLcu.App.ViewModels;

public partial class SettingsViewModel : ObservableObject
{
    private readonly AppServices _services;
    private bool _isLoading = true;

    [ObservableProperty] private bool _enablePetPlaceholder;
    [ObservableProperty] private bool _enableUiAnimations;
    [ObservableProperty] private bool _enableAtmosphereReactions;
    [ObservableProperty] private bool _enableToastNotifications;
    [ObservableProperty] private bool _enableSounds;
    [ObservableProperty] private double _soundVolume;

    public SettingsViewModel(AppServices services)
    {
        _services = services;
        var prefs = services.Preferences.Current;
        _enablePetPlaceholder = prefs.EnablePetPlaceholder;
        _enableUiAnimations = prefs.EnableUiAnimations;
        _enableAtmosphereReactions = prefs.EnableAtmosphereReactions;
        _enableToastNotifications = prefs.EnableToastNotifications;
        _enableSounds = prefs.EnableSounds;
        _soundVolume = prefs.SoundVolume;
        _isLoading = false;
    }

    partial void OnEnablePetPlaceholderChanged(bool value) =>
        Save(p => p.EnablePetPlaceholder = value);

    partial void OnEnableUiAnimationsChanged(bool value) =>
        Save(p => p.EnableUiAnimations = value);

    partial void OnEnableAtmosphereReactionsChanged(bool value) =>
        Save(p => p.EnableAtmosphereReactions = value);

    partial void OnEnableToastNotificationsChanged(bool value) =>
        Save(p => p.EnableToastNotifications = value);

    partial void OnEnableSoundsChanged(bool value)
    {
        Save(p => p.EnableSounds = value);
        _services.Audio.SetMuted(!value);
    }

    partial void OnSoundVolumeChanged(double value)
    {
        Save(p => p.SoundVolume = value);
        _services.Audio.SetVolume(value);
    }

    [RelayCommand]
    private void SimulateGameState(AppGameState state) =>
        _services.LcuStates.SetDebugGameState(state);

    private void Save(Action<InteractionPreferences> update)
    {
        if (_isLoading) return;
        _services.Preferences.Update(update);
        _services.Toasts.Request(YummiLcu.App.Infrastructure.Toasts.ToastType.Success, "Settings saved", "Interaction preferences updated.", "settings-saved");
    }
}
