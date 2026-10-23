import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import Hooks from './Hooks';
import { getHooks, hookAction, inspectHook, type HooksView } from '../../api/hooks';
vi.mock('../../api/hooks', () => ({ getHooks: vi.fn(), hookAction: vi.fn(), inspectHook: vi.fn() }));
const view: HooksView = {
  revision: 'reviewed-revision', tools: ['codex', 'claude'], hooks: [
    { hook: { id: 'required-hook', name: 'Required hook', description: 'Required processor', version: '1.0', event: 'performer_evidence', default_enabled: true, handler: { kind: 'builtin', name: 'required-hook' }, skills: [], source: { repository: 'MACC', subpath: 'hooks/required', distribution: 'bundled' } }, enabled: true, manual: null, required_by: ['O2 context engine'], integration: 'MACC execution layer' },
    { hook: { id: 'optional-hook', name: 'Optional hook', description: 'Optional processor', version: '1.0', event: 'tool_output', default_enabled: true, handler: { kind: 'builtin', name: 'optional-hook' }, skills: [], source: { repository: 'MACC', subpath: 'hooks/optional', distribution: 'bundled' } }, enabled: true, manual: null, required_by: [], integration: 'MACC execution layer' },
  ],
};
beforeEach(() => { vi.clearAllMocks(); vi.mocked(getHooks).mockResolvedValue(structuredClone(view)); });
describe('Hooks management', () => {
  it('explains locked feature claims and confirms changes before sending the reviewed revision', async () => {
    render(<Hooks />);
    expect(await screen.findByRole('checkbox', { name: 'Enable Required hook' })).toBeDisabled();
    expect(screen.getByText(/Required by O2 context engine/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('checkbox', { name: 'Enable Optional hook' }));
    expect(hookAction).not.toHaveBeenCalled();
    vi.mocked(hookAction).mockResolvedValue({ ...view, revision: 'new' });
    fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));
    await waitFor(() => expect(hookAction).toHaveBeenCalledWith({ action: 'set', id: 'optional-hook', enabled: false, revision: 'reviewed-revision' }));
  });
  it('preserves the review and shows a revision conflict without silently retrying', async () => {
    render(<Hooks />);
    fireEvent.click(await screen.findByRole('checkbox', { name: 'Enable Optional hook' }));
    vi.mocked(hookAction).mockRejectedValue(new Error('Configuration revision conflict'));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Configuration revision conflict');
    expect(screen.getByRole('alertdialog')).toBeInTheDocument();
    expect(hookAction).toHaveBeenCalledTimes(1);
  });
  it('filters discovery and reviews an imported snapshot before any mutation', async () => {
    render(<Hooks />);
    await screen.findByRole('checkbox', { name: 'Enable Optional hook' });
    fireEvent.change(screen.getByRole('textbox', { name: 'Find hooks' }), { target: { value: 'Optional' } });
    expect(screen.queryByRole('checkbox', { name: 'Enable Required hook' })).not.toBeInTheDocument();
    const definition = { ...view.hooks[1].hook, id: 'external', default_enabled: false, handler: { kind: 'rules' as const, steps: [{ kind: 'retain_lines' as const, pattern: 'warning' }] }, source: { repository: 'https://example.invalid/hooks', subpath: 'hooks/external', distribution: 'snapshot' } };
    vi.mocked(inspectHook).mockResolvedValue({ definition, sha256: 'reviewed-sha' });
    const file = new File([JSON.stringify(definition)], 'hook.json', { type: 'application/json' });
    Object.defineProperty(file, 'text', { value: async () => JSON.stringify(definition) });
    fireEvent.change(screen.getByLabelText('Import external hook.json'), { target: { files: [file] } });
    expect(await screen.findByRole('alertdialog')).toHaveTextContent('reviewed-sha');
    expect(hookAction).not.toHaveBeenCalled();
    expect(inspectHook).toHaveBeenCalledWith(definition);
  });
});

it('distinguishes configured files from provider execution and explains trust', async () => {
  vi.mocked(getHooks).mockResolvedValue({ ...view, native: [{ tool: 'codex', supported: true, configured: true, installed: true, locally_verified: true, execution_observed: false, applied: [], detail: 'Review and trust with /hooks in Codex' }] });
  render(<Hooks />);
  expect(await screen.findByRole('region', { name: 'Native hook deployments' })).toHaveTextContent('Execution observed: no');
  expect(screen.getByText('Review and trust with /hooks in Codex')).toBeInTheDocument();
});
