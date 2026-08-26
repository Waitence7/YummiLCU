import { useEffect, useState } from 'react';

import { completeTrayHide, useMockBridge } from './api/commands';
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

type TabId = 'guild' | 'settings' | 'voice' | 'logs' | 'patchNotes';

const TABS: { id: TabId; label: string; badge?: string }[] = [
  { id: 'guild', label: '내전' },
  { id: 'settings', label: '편의기능' },
  { id: 'voice', label: '음성', badge: '예정' },
  { id: 'logs', label: '로그' },
  { id: 'patchNotes', label: '패치노트', badge: '0.7.3' },
];

export function App() {
  const { state, recent, actions } = useAgentState();
  const [tab, setTab] = useState<TabId>('guild');

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
          }).catch(() => completeTrayHide().catch(() => undefined));
        }),
      )
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [state.config.TrayHideEffect, state.config.TrayEffectPlaybackRate]);

  return (
    <div data-yummi-app-surface className="yummi-window-surface flex h-full flex-col overflow-hidden bg-white text-slate-800">
      <TitleBar />
      <Header
        state={state}
        onStart={() => void actions.start()}
        onStop={() => void actions.stop()}
        onRelogin={() => void actions.relogin()}
      />
      <Banners state={state} onSubmitOAuth={actions.submitOAuth} />

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
  );
}
