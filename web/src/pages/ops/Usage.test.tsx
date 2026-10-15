import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import Usage from './Usage';
import * as api from '../../api/sessions';
vi.mock('../../api/sessions', () => ({ getUsage: vi.fn(), planAuthority: vi.fn(), applyAuthority: vi.fn() }));
beforeEach(() => vi.resetAllMocks());
it('keeps unknown counters distinct from measured zero and displays coverage', async () => {
  vi.mocked(api.getUsage).mockResolvedValue({ ready: true, records: [], totals: [{ task_id: 'TASK-A', tool_id: 'future-ai', phase: 'dev', currency: 'USD', cost_quality: 'estimated', invocations: 2, measured_invocations: 1, counters: { input_uncached: 0, cached_read: null, cached_write: null, output: 7, reasoning: null, cost: 125000000 }, coverage: { input_uncached: 1, cached_read: 0, cached_write: 0, output: 1, reasoning: 0, cost: 1 } }] });
  render(<Usage sqlite />);
  expect(await screen.findByText('TASK-A')).toBeInTheDocument();
  expect(screen.getByText('0')).toBeInTheDocument();
  expect(screen.getAllByText('Unknown')).toHaveLength(2);
  expect(screen.getByText('0.125 USD (estimated)')).toBeInTheDocument();
  expect(screen.getByText(/Input 1\/2; cache read 0\/2/)).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText('Filter usage by task, phase or tool'), { target: { value: 'other-task' } });
  expect(screen.queryByText('TASK-A')).not.toBeInTheDocument();
});
it('requires an explicit preview then applies exactly the reviewed upgrade', async () => {
  vi.mocked(api.getUsage).mockResolvedValue({ ready: false, records: [], totals: [] });
  const plan = { direction: 'sqlite_usage', source_sha256: 'reviewed', imported: 0, quarantined: 0, backup: null, applied: false };
  vi.mocked(api.planAuthority).mockResolvedValue(plan);
  vi.mocked(api.applyAuthority).mockResolvedValue({ ...plan, applied: true });
  render(<Usage sqlite />);
  fireEvent.click(await screen.findByRole('button', { name: 'Preview usage upgrade' }));
  const apply = await screen.findByRole('button', { name: 'Apply reviewed usage upgrade' });
  expect(api.applyAuthority).not.toHaveBeenCalled();
  fireEvent.click(apply);
  await waitFor(() => expect(api.applyAuthority).toHaveBeenCalledWith(plan));
});
