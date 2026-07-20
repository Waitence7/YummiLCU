import type { AgentState } from '../state/types';

export async function listenToAgentState(
  onState: (state: AgentState) => void,
): Promise<() => void> {
  const { listen } = await import('@tauri-apps/api/event');
  return listen<AgentState>('agent-state', (event) => onState(event.payload));
}
