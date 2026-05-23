using System.Text.Json;
using YummiLcu.App.Infrastructure.AppState;

namespace YummiLcu.App.Infrastructure.Settings;

public sealed class InteractionPreferencesService
{
    private static readonly JsonSerializerOptions JsonOptions = new() { WriteIndented = true };
    private readonly string _path = Path.Combine(AppContext.BaseDirectory, "yummi-interactions.json");
    private readonly AppStateManager _state;

    public InteractionPreferencesService(AppStateManager state)
    {
        _state = state;
        Current = Load();
        ApplyToState();
    }

    public event Action<InteractionPreferences>? PreferencesChanged;

    public InteractionPreferences Current { get; private set; }

    public void Update(Action<InteractionPreferences> update)
    {
        var next = Current.Clone();
        update(next);
        next.SoundVolume = Math.Clamp(next.SoundVolume, 0, 1);
        Current = next;
        ApplyToState();
        Save();
        PreferencesChanged?.Invoke(Current.Clone());
    }

    private InteractionPreferences Load()
    {
        try
        {
            if (!File.Exists(_path)) return new InteractionPreferences();
            var json = File.ReadAllText(_path);
            return JsonSerializer.Deserialize<InteractionPreferences>(json) ?? new InteractionPreferences();
        }
        catch
        {
            return new InteractionPreferences();
        }
    }

    private void Save()
    {
        try
        {
            File.WriteAllText(_path, JsonSerializer.Serialize(Current, JsonOptions));
        }
        catch
        {
            // Preferences are non-critical; keep runtime behavior safe if persistence fails.
        }
    }

    private void ApplyToState()
    {
        _state.EnablePetPlaceholder = Current.EnablePetPlaceholder;
        _state.EnableUiAnimations = Current.EnableUiAnimations;
        _state.EnableAtmosphereReactions = Current.EnableAtmosphereReactions;
        _state.EnableToastNotifications = Current.EnableToastNotifications;
        _state.EnableSounds = Current.EnableSounds;
        _state.SoundVolume = Current.SoundVolume;
    }
}
