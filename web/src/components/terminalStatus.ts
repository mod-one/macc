import type { ApiTerminalType } from '../api/models';

export type TerminalTarget = {
  terminalType: ApiTerminalType;
  label: string;
  worktreeId?: string;
};

export type TerminalSessionStatus = 'connecting' | 'connected' | 'disconnected' | 'error';

export function isReconnectableTerminalStatus(status: TerminalSessionStatus) {
  return status === 'disconnected' || status === 'error';
}
