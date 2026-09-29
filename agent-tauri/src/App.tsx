import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';

import {
  completeTrayHide,
  freezeMainWindowMotion,
  reportWindowFailure,
  stabilizeMainWindowRotation,
  syncMainWindowRotationMode,
  useMockBridge,
  windowUiHeartbeat,
} from './api/commands';
import { Banners } from './components/Banners';
import { Header } from './components/Header';
import { TitleBar } from './components/TitleBar';
import { useAgentState } from './hooks/useAgentState';
import { GuildMatchTab } from './tabs/GuildMatchTab';
import { LogsTab } from './tabs/LogsTab';
import { PatchNotesTab } from './tabs/PatchNotesTab';
import { SettingsTab } from './tabs/SettingsTab';
import { VoiceTab } from './tabs/VoiceTab';
import { playTrayHideEffect } from './trayEffects';
import { waitForCloseSound } from './closeSound';
import {
  resetWindowMotionVisual,
  updateWindowMotionVisual,
  type WindowMotionPayload,
} from './windowMotion';

type ResizeDirection =
  | 'East'
  | 'North'
  | 'NorthEast'
  | 'NorthWest'
  | 'South'
  | 'SouthEast'
  | 'SouthWest'
  | 'West';

type TabId = 'guild' | 'settings' | 'voice' | 'logs' | 'patchNotes';

const RESIZE_HANDLES: { direction: ResizeDirection; className: string }[] = [
  { direction: 'North', className: 'yummi-resize-north' },
  { direction: 'South', className: 'yummi-resize-south' },
  { direction: 'East', className: 'yummi-resize-east' },
  { direction: 'West', className: 'yummi-resize-west' },
  { direction: 'NorthEast', className: 'yummi-resize-northeast' },
  { direction: 'NorthWest', className: 'yummi-resize-northwest' },
  { direction: 'SouthEast', className: 'yummi-resize-southeast' },
  { direction: 'SouthWest', className: 'yummi-resize-southwest' },
];

function WindowResizeHandles() {
  if (useMockBridge) return null;
  return (
    <>
      {RESIZE_HANDLES.map(({ direction, className }) => (
        <div
          key={direction}
          aria-hidden
          className={`yummi-resize-handle ${className}`}
          onPointerDown={(event) => {
            if (event.button !== 0) return;
            event.preventDefault();
            void getCurrentWindow().startResizeDragging(direction).catch((error) => reportWindowFailure('resize_drag', error));
          }}
        />
      ))}
    </>
  );
}

const TABS: { id: TabId; label: string; badge?: string }[] = [
  { id: 'guild', label: '내전' },
  { id: 'settings', label: '편의기능' },
  { id: 'voice', label: '음성', badge: '예정' },
  { id: 'logs', label: '로그' },
  { id: 'patchNotes', label: '패치노트', badge: '0.7.3' },
];

export function App() {
  const { state, recent, actionError, actions } = useAgentState();
  const [tab, setTab] = useState<TabId>('guild');

  useEffect(() => {
    if (useMockBridge) return;
    void windowUiHeartbeat().catch((error) => reportWindowFailure('ui_heartbeat', error));
    const timer = window.setInterval(() => {
      void windowUiHeartbeat().catch((error) => reportWindowFailure('ui_heartbeat', error));
    }, 1_000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (useMockBridge) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void import('@tauri-apps/api/event')
      .then(({ listen }) =>
        listen('yummi://tray-hide-requested', () => {
          void playTrayHideEffect(state.config.TrayHideEffect, async () => {
            await waitForCloseSound();
            await completeTrayHide();
          }, {
            playbackRate: state.config.TrayEffectPlaybackRate,
          }).catch((error) => {
            void reportWindowFailure('tray_effect', error);
            return completeTrayHide().catch((hideError) => reportWindowFailure('complete_tray_hide', hideError));
          });
        }),
      )
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch((error) => void reportWindowFailure('tray_hide_listener', error));

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [state.config.TrayHideEffect, state.config.TrayEffectPlaybackRate]);

  useEffect(() => {
    if (useMockBridge) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void import('@tauri-apps/api/event')
      .then(({ listen }) =>
        listen<WindowMotionPayload>('yummi://window-motion', ({ payload }) => {
          updateWindowMotionVisual(payload);
        }),
      )
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch((error) => void reportWindowFailure('window_motion_listener', error));

    return () => {
      disposed = true;
      unlisten?.();
      resetWindowMotionVisual();
    };
  }, []);

  useEffect(() => {
    if (useMockBridge) return;
    void syncMainWindowRotationMode().catch((error) => reportWindowFailure('sync_rotation', error));
  }, [state.config.WindowFreeRotation]);

  return (
    <div
      className="yummi-window-host"
      onPointerDownCapture={(event) => {
        if (useMockBridge) return;
        // Title-bar presses start a new drag, and start_main_window_drag already
        // invalidates any previous inertia. Everything else freezes immediately.
        if ((event.target as HTMLElement).closest('[data-yummi-drag-handle]') &&
            !(event.target as HTMLElement).closest('[data-yummi-no-drag]')) return;
        void freezeMainWindowMotion().catch((error) => reportWindowFailure('freeze_motion', error));
      }}
      onContextMenu={(event) => {
        if (useMockBridge || !state.config.WindowFreeRotation) return;
        if (!(event.target as HTMLElement).closest('[data-yummi-app-surface]')) return;
        event.preventDefault();
        void stabilizeMainWindowRotation().catch((error) => reportWindowFailure('stabilize_rotation', error));
      }}
    >
      <WindowResizeHandles />
      <div data-yummi-app-surface className="yummi-window-surface relative isolate flex flex-col overflow-hidden bg-white text-slate-800">
      <div aria-hidden className="yummi-motion-ghost yummi-motion-ghost-1" />
      <div aria-hidden className="yummi-motion-ghost yummi-motion-ghost-2" />
      <div aria-hidden className="yummi-motion-ghost yummi-motion-ghost-3" />
      <div aria-hidden className="yummi-motion-streaks" />
      <TitleBar />
      <Header
        state={state}
        onStart={() => void actions.start()}
        onStop={() => void actions.stop()}
        onLogout={() => void actions.logout()}
        actionError={actionError?.startsWith('설정 저장 실패:') ? null : actionError}
      />
      <Banners state={state} />

      <nav className="flex gap-1 border-b border-slate-200 bg-white px-3 pt-2">
        {TABS.map(({ id, label, badge }) => (
          <button
            key={id}
            type="button"
            onClick={() => setTab(id)}
            className={`relative rounded-t-lg px-3.5 py-2 text-[12px] font-medium transition-colors ${
              tab === id
                ? 'bg-slate-100 text-slate-900 shadow-[inset_0_2px_0_#6366f1]'
                : 'text-slate-500 hover:text-slate-700'
            }`}
          >
            {label}
            {badge && (
              <span className="ml-1.5 rounded-full bg-indigo-50 px-1.5 py-px text-[10px] text-indigo-600">
                {badge}
              </span>
            )}
          </button>
        ))}
      </nav>

      <main className="min-h-0 flex-1 overflow-y-auto bg-slate-50 p-3">
        {tab === 'guild' && (
          <GuildMatchTab
            state={state}
            recent={recent}
            onRefreshRecent={() => void actions.refreshRecent()}
            onPatchConfig={(patch) => void actions.patchConfig(patch)}
          />
        )}
        {tab === 'settings' && (
          <SettingsTab
            config={state.config}
            currentReleaseLabel={state.release_label ?? state.app_version ?? '—'}
            currentBuildId={state.build_id ?? '—'}
            currentReleaseChannel={state.release_channel ?? state.config.UpdateChannel}
            onPatchConfig={actions.patchConfig}
            actionError={actionError?.startsWith('설정 저장 실패:') ? actionError : null}
          />
        )}
        {tab === 'voice' && <VoiceTab />}
        {tab === 'logs' && (
          <LogsTab
            logs={state.logs}
            onGetDiagnostics={actions.getDiagnostics}
            onExportDiagnostics={actions.exportDiagnostics}
          />
        )}
        {tab === 'patchNotes' && <PatchNotesTab />}
      </main>

      <footer className="flex items-center justify-between border-t border-slate-200 bg-white px-4 py-1.5 text-[10px] text-slate-500">
        <span>
          v{state.release_label || state.app_version || '—'}
          {state.build_id && state.build_id !== 'local' ? ` · build ${state.build_id}` : ''}
        </span>
        <span>
          다운로드{' '}
          {state.downloaded_at
            ? new Date(state.downloaded_at * 1000).toLocaleString('ko-KR')
            : '—'}
        </span>
      </footer>
      </div>
    </div>
  );
}
