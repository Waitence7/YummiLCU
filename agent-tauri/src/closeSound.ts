import closeSoundUrl from './assets/close-sound.ogg?url';

let activeAudio: HTMLAudioElement | null = null;
let activePlayback: Promise<void> | null = null;

/** Start the close sound and keep a completion handle so WebView destruction can wait for it. */
export function startCloseSound(): Promise<void> {
  if (activePlayback) return activePlayback;

  const audio = new Audio(closeSoundUrl);
  audio.preload = 'auto';
  audio.volume = 0.75;
  activeAudio = audio;

  activePlayback = new Promise<void>((resolve) => {
    let settled = false;
    const cleanup = () => {
      if (settled) return;
      settled = true;
      if (activeAudio === audio) activeAudio = null;
      activePlayback = null;
      resolve();
    };
    audio.addEventListener('ended', cleanup, { once: true });
    audio.addEventListener('error', cleanup, { once: true });
    void audio.play().catch(cleanup);
  });
  return activePlayback;
}

export function waitForCloseSound(): Promise<void> {
  return activePlayback ?? Promise.resolve();
}
