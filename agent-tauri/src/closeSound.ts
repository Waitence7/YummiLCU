import closeSoundUrl from './assets/close-sound.ogg?url';

let activeAudio: HTMLAudioElement | null = null;

export function startCloseSound(): void {
  if (activeAudio) {
    activeAudio.pause();
    activeAudio.currentTime = 0;
  }

  const audio = new Audio(closeSoundUrl);
  audio.preload = 'auto';
  activeAudio = audio;

  const cleanup = () => {
    if (activeAudio === audio) activeAudio = null;
  };
  audio.addEventListener('ended', cleanup, { once: true });
  audio.addEventListener('error', cleanup, { once: true });
  void audio.play().catch(cleanup);
}
