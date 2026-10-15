import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import Sessions from './Sessions';
import * as api from '../../api/sessions';
import type { SessionStoreView, MigrationPlan } from '../../api/sessions';
vi.mock('../../api/sessions', () => ({ getSessions: vi.fn(), savePolicy: vi.fn(), previewPolicy: vi.fn(), planAuthority: vi.fn(), applyAuthority: vi.fn(), sessionAction: vi.fn(), getSessionEvents: vi.fn(), getCheckpoint: vi.fn(), previewReconciliation: vi.fn(), applyReconciliation: vi.fn(), getUsage: vi.fn() }));
const view: SessionStoreView = {
  project_root: '/test/project', backend: 'json', project_id: null, revision: 'original', policy_source: 'defaults',
  policy: { lease: { ttl_seconds: 1800, heartbeat_seconds: 30 }, lifecycle: { max_idle_hours: 24 }, checkpoints: { max_content_bytes: 16384, rotate_after_uses: 20 }, usage: { capture_mode: 0, max_output_bytes: 1048576 } }, sessions: [], checkpoints_ready: false, checkpoints: [],
  fields: [
    { path: 'lease.ttl_seconds', label: 'Lease TTL', default: 1800, min: 10, max: 86400, unit: 'seconds', help: 'Lease validity', activation: 'next claim' },
    { path: 'lease.heartbeat_seconds', label: 'Heartbeat interval', default: 30, min: 1, max: 28800, unit: 'seconds', help: 'At most TTL/3', activation: 'next claim' },
    { path: 'lifecycle.max_idle_hours', label: 'Maximum idle age', default: 24, min: 1, max: 720, unit: 'hours', help: 'Idle sessions', activation: 'next claim' },
    { path: 'checkpoints.max_content_bytes', label: 'Checkpoint size limit', default: 16384, min: 2048, max: 65536, unit: 'bytes', help: 'Bounded context', activation: 'next claim' },
    { path: 'checkpoints.rotate_after_uses', label: 'Rotate after invocations', default: 20, min: 1, max: 1000, unit: 'invocations', help: 'Fresh context', activation: 'next claim' },
    { path: 'usage.capture_mode', label: 'Usage capture mode', default: 0, min: 0, max: 1, unit: 'mode', help: 'Observed or structured', activation: 'next call' },
    { path: 'usage.max_output_bytes', label: 'Usage parse limit', default: 1048576, min: 4096, max: 16777216, unit: 'bytes', help: 'Bounded capture', activation: 'next call' },
  ],
};
beforeEach(() => {
  window.sessionStorage.clear(); vi.clearAllMocks(); vi.mocked(api.getSessions).mockResolvedValue(structuredClone(view));
  vi.mocked(api.getUsage).mockResolvedValue({ ready: false, records: [], totals: [] });
  vi.mocked(api.previewPolicy).mockResolvedValue({ validated: true });
  vi.mocked(api.savePolicy).mockResolvedValue({ ...view, revision: 'saved' });
});
describe('Session administration', () => {
  it('edits every catalog field and previews before a revision-bound save', async () => {
    render(<Sessions />);
    const ttl = await screen.findByLabelText('Lease TTL (seconds)');
    fireEvent.change(ttl, { target: { value: '60' } });
    fireEvent.change(screen.getByLabelText('Heartbeat interval (seconds)'), { target: { value: '10' } });
    fireEvent.change(screen.getByLabelText('Maximum idle age (hours)'), { target: { value: '48' } });
    fireEvent.change(screen.getByLabelText('Checkpoint size limit (bytes)'), { target: { value: '8192' } });
    fireEvent.change(screen.getByLabelText('Rotate after invocations (invocations)'), { target: { value: '5' } });
    fireEvent.change(screen.getByLabelText('Usage capture mode (mode)'), { target: { value: '1' } });
    fireEvent.change(screen.getByLabelText('Usage parse limit (bytes)'), { target: { value: '8192' } });
    expect(screen.getByRole('button', { name: 'Apply policy' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Validate and preview' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Apply policy' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: 'Apply policy' }));
    await waitFor(() => expect(api.savePolicy).toHaveBeenCalledWith('original', { lease: { ttl_seconds: 60, heartbeat_seconds: 10 }, lifecycle: { max_idle_hours: 48 }, checkpoints: { max_content_bytes: 8192, rotate_after_uses: 5 }, usage: { capture_mode: 1, max_output_bytes: 8192 } }));
  });
  it('preserves an edited draft when the server reports a revision conflict', async () => {
    vi.mocked(api.savePolicy).mockRejectedValue(new Error('Configuration revision conflict'));
    render(<Sessions />);
    const ttl = await screen.findByLabelText('Lease TTL (seconds)');
    fireEvent.change(ttl, { target: { value: '900' } });
    fireEvent.click(screen.getByRole('button', { name: 'Validate and preview' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Apply policy' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: 'Apply policy' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('revision conflict');
    expect(ttl).toHaveValue(900);
  });
  it('keeps migration preview separate from applying the reviewed source hash', async () => {
    const plan: MigrationPlan = { direction: 'json_to_sqlite', source_sha256: 'evidence-hash', imported: 3, quarantined: 1, backup: null, applied: false };
    vi.mocked(api.planAuthority).mockResolvedValue(plan);
    vi.mocked(api.applyAuthority).mockResolvedValue({ ...plan, applied: true, backup: '/project/.macc/state/backup.json' });
    render(<Sessions />);
    fireEvent.click(await screen.findByRole('button', { name: 'Preview migration to SQLite' }));
    await screen.findByRole('button', { name: 'Apply reviewed migration' });
    expect(api.applyAuthority).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Apply reviewed migration' }));
    await waitFor(() => expect(api.applyAuthority).toHaveBeenCalledWith(plan));
  });
  it('restores an unsaved draft after navigating away and returning', async () => {
    const first = render(<Sessions />); const ttl = await screen.findByLabelText('Lease TTL (seconds)');
    fireEvent.change(ttl, { target: { value: '900' } });
    await waitFor(() => expect(window.sessionStorage.getItem('macc.session-policy:/test/project')).toContain('900'));
    first.unmount(); render(<Sessions />);
    expect(await screen.findByLabelText('Lease TTL (seconds)')).toHaveValue(900);
  });
  it('removes an override with explicit null instead of persisting copied defaults', async () => {
    render(<Sessions />); await screen.findByLabelText('Lease TTL (seconds)');
    fireEvent.click(screen.getByRole('button', { name: 'Remove project override' }));
    fireEvent.click(screen.getByRole('button', { name: 'Validate and preview' }));
    await waitFor(() => expect(api.previewPolicy).toHaveBeenCalledWith('original', null));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Apply policy' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: 'Apply policy' }));
    await waitFor(() => expect(api.savePolicy).toHaveBeenCalledWith('original', null));
  });
});

it('reviews checkpoint reconciliation before applying its current source hash', async () => {
  const checkpoint = { id: 'cp-a', task_id: 'A', role: 'performer', tool_id: 'future-ai', phase: 'fix', reason: 'invocation_finished', validation_state: 'needs_reconciliation', created_at: 1, content_sha256: 'saved' };
  vi.mocked(api.getSessions).mockResolvedValue({ ...view, backend: 'sqlite', checkpoints_ready: true, checkpoints: [checkpoint] });
  vi.mocked(api.getCheckpoint).mockResolvedValue({ task_id: 'A', blocking_issues: ['Tests failed'] });
  vi.mocked(api.previewReconciliation).mockResolvedValue({ previous: {}, current: { task_id: 'A' }, current_sha256: 'reviewed-current', applied: false });
  render(<Sessions />);
  fireEvent.click(await screen.findByRole('button', { name: 'Inspect A checkpoint' }));
  await screen.findByText(/Tests failed/);
  fireEvent.click(screen.getByRole('button', { name: 'Preview reconciliation' }));
  const apply = await screen.findByRole('button', { name: 'Apply reviewed reconciliation' });
  expect(api.applyReconciliation).not.toHaveBeenCalled(); fireEvent.click(apply);
  await waitFor(() => expect(api.applyReconciliation).toHaveBeenCalledWith('cp-a', 'reviewed-current'));
});
it('requires a reviewed upgrade before enabling checkpoints on existing SQLite', async () => {
  vi.mocked(api.getSessions).mockResolvedValue({ ...view, backend: 'sqlite', checkpoints_ready: false });
  const plan = { direction: 'sqlite_checkpoints', source_sha256: 'reviewed', imported: 0, quarantined: 0, backup: null, applied: false };
  vi.mocked(api.planAuthority).mockResolvedValue(plan);
  render(<Sessions />); fireEvent.click(await screen.findByRole('button', { name: 'Preview checkpoint upgrade' }));
  const apply = await screen.findByRole('button', { name: 'Apply reviewed checkpoint upgrade' });
  expect(api.applyAuthority).not.toHaveBeenCalled(); fireEvent.click(apply);
  await waitFor(() => expect(api.applyAuthority).toHaveBeenCalledWith(plan));
});
