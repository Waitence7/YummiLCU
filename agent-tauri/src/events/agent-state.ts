import { useMockBridge } from '../api/commands';
import type { AgentState } from '../state/types';

export async function listenToAgentState(
  onState: (state: AgentState) => void,
): Promise<() => void> {
  if (useMockBridge) {
    const { mockListen } = await import('../api/mock');
    return mockListen(onState);
  }
  const { listen } = await import('@tauri-apps/api/event');
  return listen<AgentState>('agent-state', (event) => onState(event.payload));
}
