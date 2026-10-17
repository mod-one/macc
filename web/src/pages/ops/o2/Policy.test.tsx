import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import Policy from './Policy';
import { o2Action, type O2View } from '../../../api/o2';
vi.mock('../../../api/o2', () => ({ o2Action: vi.fn() }));
const view: O2View = { project_root: '/repo', revision: 'original', source: 'project', sqlite: true, ready: true, policy: { memory: { enabled: false, max_body_chars: 1200 } }, fields: [{ path: 'memory.enabled', label: 'Memory Ledger', kind: 'bool', min: 0, max: 1, default: false, choices: [], activation: 'next call' }, { path: 'memory.max_body_chars', label: 'Maximum memory characters', kind: 'number', min: 120, max: 12000, default: 1200, choices: [], activation: 'next call' }] };
beforeEach(() => { sessionStorage.clear(); vi.resetAllMocks(); });
it('requires preview and sends the reviewed revision and typed values', async () => {
  vi.mocked(o2Action).mockImplementation(async action => action === 'policy_apply' ? { ...view, revision: 'saved', policy: { memory: { enabled: true, max_body_chars: 1400 } } } : { validated: true });
  render(<Policy view={view} saved={vi.fn()} />);
  fireEvent.click(screen.getByLabelText('Memory Ledger'));
  fireEvent.change(screen.getByLabelText('Maximum memory characters'), { target: { value: '1400' } });
  expect(screen.getByRole('button', { name: 'Apply O2 policy' })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: 'Validate and preview O2 policy' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Apply O2 policy' })).toBeEnabled());
  fireEvent.click(screen.getByRole('button', { name: 'Apply O2 policy' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('policy_apply', { revision: 'original', policy: { memory: { enabled: true, max_body_chars: 1400 } } }));
});
it('preserves the draft and original revision after a conflict', async () => {
  vi.mocked(o2Action).mockImplementation(async action => { if (action === 'policy_apply') throw new Error('revision conflict'); return { validated: true }; });
  const first = render(<Policy view={view} saved={vi.fn()} />);
  fireEvent.change(screen.getByLabelText('Maximum memory characters'), { target: { value: '1500' } });
  fireEvent.click(screen.getByRole('button', { name: 'Validate and preview O2 policy' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Apply O2 policy' })).toBeEnabled());
  fireEvent.click(screen.getByRole('button', { name: 'Apply O2 policy' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('revision conflict');
  first.unmount(); render(<Policy view={{ ...view, revision: 'new-server-revision' }} saved={vi.fn()} />);
  expect(screen.getByLabelText('Maximum memory characters')).toHaveValue(1500);
  expect(sessionStorage.getItem('macc.o2.policy:/repo')).toContain('original');
});
it('filters by canonical path and preserves an override-removal draft', async () => {
  const first = render(<Policy view={view} saved={vi.fn()} />);
  fireEvent.change(screen.getByLabelText('Find a setting'), { target: { value: 'max_body_chars' } });
  expect(screen.queryByLabelText('Memory Ledger')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Remove O2 project overrides' }));
  await waitFor(() => expect(sessionStorage.getItem('macc.o2.policy:/repo')).toContain('"inherit":true'));
  first.unmount(); render(<Policy view={view} saved={vi.fn()} />);
  expect(screen.getByText(/Preview will remove/)).toBeInTheDocument();
});

it('ignores an obsolete preview after the user changes the draft', async () => {
  let resolvePreview!: (value: unknown) => void;
  vi.mocked(o2Action).mockImplementation(() => new Promise(resolve => { resolvePreview = resolve; }));
  render(<Policy view={view} saved={vi.fn()} />);
  fireEvent.change(screen.getByLabelText('Maximum memory characters'), { target: { value: '1400' } });
  fireEvent.click(screen.getByRole('button', { name: 'Validate and preview O2 policy' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalled());
  fireEvent.change(screen.getByLabelText('Maximum memory characters'), { target: { value: '1800' } });
  await act(async () => resolvePreview({ validated: true }));
  expect(screen.getByLabelText('Maximum memory characters')).toHaveValue(1800);
  expect(screen.getByRole('button', { name: 'Apply O2 policy' })).toBeDisabled();
});
it('applies the reviewed snapshot and retains edits made while saving', async () => {
  let resolveApply!: (value: unknown) => void;
  vi.mocked(o2Action).mockImplementation(async action => action === 'policy_apply' ? new Promise(resolve => { resolveApply = resolve; }) : { validated: true });
  render(<Policy view={view} saved={vi.fn()} />);
  fireEvent.change(screen.getByLabelText('Maximum memory characters'), { target: { value: '1400' } });
  fireEvent.click(screen.getByRole('button', { name: 'Validate and preview O2 policy' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Apply O2 policy' })).toBeEnabled());
  fireEvent.click(screen.getByRole('button', { name: 'Apply O2 policy' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('policy_apply', { revision: 'original', policy: { memory: { enabled: false, max_body_chars: 1400 } } }));
  fireEvent.change(screen.getByLabelText('Maximum memory characters'), { target: { value: '1800' } });
  await act(async () => resolveApply({ ...view, revision: 'saved', policy: { memory: { enabled: false, max_body_chars: 1400 } } }));
  expect(screen.getByLabelText('Maximum memory characters')).toHaveValue(1800);
  expect(screen.getByRole('button', { name: 'Apply O2 policy' })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: 'Validate and preview O2 policy' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('policy_preview', { revision: 'saved', policy: { memory: { enabled: false, max_body_chars: 1800 } } }));
});

it('edits executable paths as text in the shared policy catalog', async () => {
  vi.mocked(o2Action).mockResolvedValue({ validated: true });
  const config: O2View = { ...view, policy: { code_intelligence: { graphify_python: 'python3' } }, fields: [{ path: 'code_intelligence.graphify_python', label: 'Graphify Python executable', kind: 'string', min: 0, max: 0, choices: [], default: 'python3', activation: 'next query' }] };
  render(<Policy view={config} saved={vi.fn()} />);
  fireEvent.change(screen.getByLabelText('Graphify Python executable'), { target: { value: '/usr/bin/python3' } });
  fireEvent.click(screen.getByRole('button', { name: 'Validate and preview O2 policy' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('policy_preview', { revision: 'original', policy: { code_intelligence: { graphify_python: '/usr/bin/python3' } } }));
});

it('filters domain controls without removing other draft values from preview', async () => {
  render(<Policy view={{ ...view, policy: { ...view.policy, code_intelligence: { enabled: true } } }} saved={vi.fn()} domainFilter="memory" />);
  fireEvent.click(screen.getByLabelText('Memory Ledger'));
  fireEvent.click(screen.getByRole('button', { name: 'Validate and preview O2 policy' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('policy_preview', { revision: 'original', policy: { memory: { enabled: true, max_body_chars: 1200 }, code_intelligence: { enabled: true } } }));
});

it('previews native map opt-in and parser limits using typed shared fields', async () => {
  vi.mocked(o2Action).mockResolvedValue({ validated: true });
  const config: O2View = { ...view, policy: { code_intelligence: { native_map_enabled: false, native_symbols_per_file: 1000 } }, fields: [
    { path: 'code_intelligence.native_map_enabled', label: 'Enable native parsed symbol map', kind: 'bool', min: 0, max: 1, choices: [], default: false, activation: 'next query' },
    { path: 'code_intelligence.native_symbols_per_file', label: 'Native symbols per file limit', kind: 'number', min: 1, max: 10000, choices: [], default: 1000, activation: 'next query' },
  ] };
  render(<Policy view={config} saved={vi.fn()} domainFilter="code_intelligence" />);
  fireEvent.click(screen.getByLabelText('Enable native parsed symbol map'));
  fireEvent.change(screen.getByLabelText('Native symbols per file limit'), { target: { value: '500' } });
  fireEvent.click(screen.getByRole('button', { name: 'Validate and preview O2 policy' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('policy_preview', { revision: 'original', policy: { code_intelligence: { native_map_enabled: true, native_symbols_per_file: 500 } } }));
});
