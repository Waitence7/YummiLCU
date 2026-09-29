import { useEffect, useRef, useState } from 'react';

import * as api from '../api/commands';
import { startCloseSound } from '../closeSound';
import type { WindowMotionPayload } from '../windowMotion';
import { WindowGlideHint } from './WindowGlideHint';
import appIcon from '../../src-tauri/icons/icon.ico';

const WINDOW_GLIDE_HINT_KEY = 'yummi-window-glide-hint-v1';
const WINDOW_GLIDE_HINT_IDLE_DELAY_MS = 250;

function shouldShowWindowGlideHint() {
  if (api.useMockBridge) return false;
  try {
    return localStorage.getItem(WINDOW_GLIDE_HINT_KEY) !== '1';
  } catch {
    // Storage can be disabled; still let this session show the hint.
    return true;
  }
}

function markWindowGlideHintShown() {
  try {
    localStorage.setItem(WINDOW_GLIDE_HINT_KEY, '1');
  } catch {
    // Storage can be disabled; showing the hint is still useful for this session.
  }
}

export function TitleBar() {
  const [showGlideHint, setShowGlideHint] = useState(false);
  const pendingGlideHint = useRef(false);
  const idleTimer = useRef<number | null>(null);

  useEffect(() => {
    if (api.useMockBridge) return;

    let disposed = false;
    let unlisten: (() => void) | undefined;

    const cancelIdleTimer = () => {
      if (idleTimer.current !== null) {
        window.clearTimeout(idleTimer.current);
        idleTimer.current = null;
      }
    };

    void import('@tauri-apps/api/event')
      .then(({ listen }) =>
        listen<WindowMotionPayload>('yummi://window-motion', ({ payload }) => {
          if (!pendingGlideHint.current) return;

          // Never mount the first-use help while the native window is moving.
          // A short idle delay also filters the transient "stop" emitted while
          // switching into the expanded rotation host before inertia begins.
          if (payload.phase !== 'stop') {
            cancelIdleTimer();
            return;
          }

          cancelIdleTimer();
          idleTimer.current = window.setTimeout(() => {
            idleTimer.current = null;
            if (disposed || !pendingGlideHint.current) return;
            pendingGlideHint.current = false;
            markWindowGlideHintShown();
            setShowGlideHint(true);
          }, WINDOW_GLIDE_HINT_IDLE_DELAY_MS);
        }),
      )
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch((error) => void api.reportWindowFailure('glide_hint_motion_listener', error));

    return () => {
      disposed = true;
      cancelIdleTimer();
      unlisten?.();
    };
  }, []);

  return (
    <div
      data-yummi-drag-handle
      className="relative flex h-10 shrink-0 cursor-move items-center border-b border-white/10 bg-[#2b2b2b] text-slate-100"
      onMouseDown={(event) => {
        if (event.button !== 0 || (event.target as HTMLElement).closest('button, [data-yummi-no-drag]')) return;
        event.preventDefault();

        // The old first-use path mounted the hint synchronously on mouse-down,
        // which made React/config work overlap the first native physics drag.
        // Only reserve the hint here; the motion listener renders it once the
        // drag/inertia sequence has actually gone idle.
        if (shouldShowWindowGlideHint()) {
          pendingGlideHint.current = true;
        }

        void api.startMainWindowDrag().catch((error) => {
          pendingGlideHint.current = false;
          void api.reportWindowFailure('start_drag', error);
        });
      }}
    >
      <div className="flex min-w-0 flex-1 items-center gap-2.5 px-3">
        <img src={appIcon} alt="" className="size-5 shrink-0" draggable={false} />
        <span className="truncate text-[13px] font-medium tracking-[0.01em]">
          Yummi LCU Agent
        </span>
      </div>
      <div className="flex h-full shrink-0" aria-label="창 제어">
        <button
          type="button"
          aria-label="최소화"
          title="최소화"
          className="grid h-full w-11 place-items-center text-slate-300 transition-colors hover:bg-white/10 hover:text-white"
          onClick={() => void api.minimizeMainWindow().catch((error) => api.reportWindowFailure('minimize', error))}
        >
          <span className="block h-px w-3.5 bg-current" />
        </button>
        <button
          type="button"
          aria-label="트레이로 보내기"
          title="트레이로 보내기"
          className="grid h-full w-11 place-items-center text-slate-300 transition-colors hover:bg-[#c42b1c] hover:text-white"
          onClick={() => {
            void startCloseSound();
            void api.requestTrayHide().catch((error) => api.reportWindowFailure('tray_hide', error));
          }}
        >
          <span className="relative block size-3.5 before:absolute before:top-1.5 before:left-[-1px] before:h-px before:w-4 before:rotate-45 before:bg-current after:absolute after:top-1.5 after:left-[-1px] after:h-px after:w-4 after:-rotate-45 after:bg-current" />
        </button>
      </div>
      {showGlideHint && (
        <div data-yummi-no-drag className="absolute right-2 top-11 z-50 h-[190px] w-[360px] max-w-[calc(100vw-1rem)] cursor-default">
          <WindowGlideHint onClose={() => setShowGlideHint(false)} />
        </div>
      )}
    </div>
  );
}
