using YummiLcu.App.Contracts.Audio;
using YummiLcu.App.Infrastructure.Events;
using YummiLcu.App.Infrastructure.Lcu;

namespace YummiLcu.App.Infrastructure.Audio;

public sealed class AudioManager : IAudioSystem
{
    private readonly IEventBus _events;
    private IDisposable? _gameStateSubscription;

    public AudioManager(IEventBus events) => _events = events;

    public bool IsMuted { get; private set; } = true;

    public double Volume { get; private set; } = 0.5;

    public void Start()
    {
        if (_gameStateSubscription is not null) return;
        _gameStateSubscription = _events.Subscribe<AppGameStateChangedEvent>(e => PlayStateChanged(e.State));
    }

    public void Stop()
    {
        _gameStateSubscription?.Dispose();
        _gameStateSubscription = null;
    }

    public void SetMuted(bool isMuted) => IsMuted = isMuted;

    public void SetVolume(double volume) => Volume = Math.Clamp(volume, 0, 1);

    public void PlayHover() => PlayCue(AudioCue.Hover);

    public void PlayClick() => PlayCue(AudioCue.Click);

    public void PlayNotification() => PlayCue(AudioCue.Notification);

    public void PlayMatchFound() => PlayCue(AudioCue.MatchFound);

    public void PlayStateChanged(AppGameState state)
    {
        if (state == AppGameState.MatchFound)
            PlayMatchFound();
        else if (state is AppGameState.Queue or AppGameState.ChampionSelect or AppGameState.EndOfGame)
            PlayNotification();
    }

    private void PlayCue(AudioCue cue)
    {
        if (IsMuted) return;
        // No bundled audio assets yet. Future implementation should resolve cue files here
        // and return quietly when a file is missing.
        _ = cue;
    }
}
