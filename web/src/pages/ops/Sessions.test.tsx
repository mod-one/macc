import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import Sessions from './Sessions';
import * as api from '../../api/sessions';
import type { SessionStoreView, MigrationPlan } from '../../api/sessions';
vi.mock('../../api/sessions', () => ({ getSessions: vi.fn(), savePolicy: vi.fn(), previewPolicy: vi.fn(), planAuthority: vi.fn(), applyAuthority: vi.fn(), sessionAction: vi.fn(), getSessionEvents: vi.fn() }));
const view: SessionStoreView = {
  project_root: '/test/project', backend: 'json', project_id: null, revision: 'original', policy_source: 'defaults',
  policy: { lease: { ttl_seconds: 1800, heartbeat_seconds: 30 }, lifecycle: { max_idle_hours: 24 } }, sessions: [],
  fields: [
    { path: 'lease.ttl_seconds', label: 'Lease TTL', default: 1800, min: 10, max: 86400, unit: 'seconds', help: 'Lease validity', activation: 'next claim' },
    { path: 'lease.heartbeat_seconds', label: 'Heartbeat interval', default: 30, min: 1, max: 28800, unit: 'seconds', help: 'At most TTL/3', activation: 'next claim' },
    { path: 'lifecycle.max_idle_hours', label: 'Maximum idle age', default: 24, min: 1, max: 720, unit: 'hours', help: 'Idle sessions', activation: 'next claim' },
  ],
};
beforeEach(() => {
  window.sessionStorage.clear(); vi.clearAllMocks(); vi.mocked(api.getSessions).mockResolvedValue(structuredClone(view));
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
    expect(screen.getByRole('button', { name: 'Apply policy' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Validate and preview' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Apply policy' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: 'Apply policy' }));
    await waitFor(() => expect(api.savePolicy).toHaveBeenCalledWith('original', { lease: { ttl_seconds: 60, heartbeat_seconds: 10 }, lifecycle: { max_idle_hours: 48 } }));
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
