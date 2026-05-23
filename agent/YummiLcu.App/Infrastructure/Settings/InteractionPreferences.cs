namespace YummiLcu.App.Infrastructure.Settings;

public sealed class InteractionPreferences
{
    public bool EnablePetPlaceholder { get; set; } = true;
    public bool EnableUiAnimations { get; set; } = true;
    public bool EnableAtmosphereReactions { get; set; } = true;
    public bool EnableToastNotifications { get; set; } = true;
    public bool EnableSounds { get; set; }
    public double SoundVolume { get; set; } = 0.5;

    public InteractionPreferences Clone() => new()
    {
        EnablePetPlaceholder = EnablePetPlaceholder,
        EnableUiAnimations = EnableUiAnimations,
        EnableAtmosphereReactions = EnableAtmosphereReactions,
        EnableToastNotifications = EnableToastNotifications,
        EnableSounds = EnableSounds,
        SoundVolume = SoundVolume,
    };
}
