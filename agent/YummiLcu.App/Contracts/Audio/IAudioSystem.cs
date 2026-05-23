using YummiLcu.App.Infrastructure.Lcu;

namespace YummiLcu.App.Contracts.Audio;

public interface IAudioSystem
{
    bool IsMuted { get; }
    double Volume { get; }
    void SetMuted(bool isMuted);
    void SetVolume(double volume);
    void PlayHover();
    void PlayClick();
    void PlayNotification();
    void PlayMatchFound();
    void PlayStateChanged(AppGameState state);
}
