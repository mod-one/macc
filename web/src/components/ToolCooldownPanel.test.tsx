import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ToolCooldownPanel } from './ToolCooldownPanel';

const api = vi.hoisted(() => ({ get: vi.fn(), clear: vi.fn() }));
vi.mock('../api/client', () => ({
  getToolCooldowns: api.get,
  clearToolCooldown: api.clear,
  setToolCooldown: vi.fn(),
  getConfig: vi.fn(),
}));

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-09-29T15:00:00Z'));
  api.get.mockResolvedValue({ tool_cooldowns: [{ tool_id: 'codex', remaining_seconds: 3600,
    throttled_until: Math.floor(Date.now() / 1000) + 3600, backoff_seconds: 3600 }] });
  api.clear.mockResolvedValue({});
});
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); vi.clearAllMocks(); });

it('updates the countdown from the absolute expiry', async () => {
  await act(async () => { render(<ToolCooldownPanel tools={['codex']} />); });
  await act(async () => { vi.advanceTimersByTime(2000); });
  expect(screen.getByText('59m 58s')).toBeInTheDocument();
});

it('confirms before resetting the provider cooldown', async () => {
  const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false);
  await act(async () => { render(<ToolCooldownPanel tools={['codex']} />); });
  fireEvent.click(screen.getByRole('button', { name: 'Reset cooldown' }));
  expect(api.clear).not.toHaveBeenCalled();
  confirm.mockReturnValue(true);
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Reset cooldown' })); });
  expect(api.clear).toHaveBeenCalledWith('codex');
});
