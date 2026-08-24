import { useState } from 'react';

import { Banners } from './components/Banners';
import { Header } from './components/Header';
import { useAgentState } from './hooks/useAgentState';
import { GuildMatchTab } from './tabs/GuildMatchTab';
import { LogsTab } from './tabs/LogsTab';
import { SettingsTab } from './tabs/SettingsTab';
import { VoiceTab } from './tabs/VoiceTab';

type TabId = 'guild' | 'settings' | 'voice' | 'logs';

const TABS: { id: TabId; label: string; badge?: string }[] = [
  { id: 'guild', label: '내전' },
  { id: 'settings', label: '편의기능' },
  { id: 'voice', label: '음성', badge: '예정' },
  { id: 'logs', label: '로그' },
];

export function App() {
  const { state, recent, actions } = useAgentState();
  const [tab, setTab] = useState<TabId>('guild');

  return (
    <div className="flex h-full flex-col bg-white text-slate-800">
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
          <SettingsTab config={state.config} onPatchConfig={actions.patchConfig} />
        )}
        {tab === 'voice' && <VoiceTab />}
        {tab === 'logs' && (
          <LogsTab
            logs={state.logs}
            onGetDiagnostics={actions.getDiagnostics}
            onExportDiagnostics={actions.exportDiagnostics}
          />
        )}
      </main>

      <footer className="flex items-center justify-between border-t border-slate-200 bg-white px-4 py-1.5 text-[10px] text-slate-500">
        <span>v{state.app_version || '—'}</span>
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
