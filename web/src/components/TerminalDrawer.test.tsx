import { describe, expect, it } from 'vitest';
import { isReconnectableTerminalStatus } from './terminalStatus';

describe('isReconnectableTerminalStatus', () => {
  it('treats disconnected and error sessions as reconnectable', () => {
    expect(isReconnectableTerminalStatus('disconnected')).toBe(true);
    expect(isReconnectableTerminalStatus('error')).toBe(true);
  });

  it('keeps connected and connecting sessions stable', () => {
    expect(isReconnectableTerminalStatus('connected')).toBe(false);
    expect(isReconnectableTerminalStatus('connecting')).toBe(false);
  });
});
