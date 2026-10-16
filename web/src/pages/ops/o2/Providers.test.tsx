import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import Providers from './Providers';
import { o2Action } from '../../../api/o2';
vi.mock('../../../api/o2', () => ({ o2Action: vi.fn() }));
it('applies only after inspecting and confirming the returned plan hash', async () => {
  vi.mocked(o2Action).mockImplementation(async action => action === 'provider_plan' ? { result: { outcome: 'supported', data: { id: 'plan-1', affected_paths: ['owned-index'], network: false } }, checksum: 'reviewed-sha' } : action === 'providers' ? { providers: [], receipts: [], plans: [] } : { state: 'deployed' });
  render(<Providers />);
  fireEvent.click(screen.getByRole('button', { name: 'Preview provider operation' }));
  const apply = await screen.findByRole('button', { name: 'Apply reviewed provider operation' });
  expect(apply).toBeDisabled();
  expect(o2Action).not.toHaveBeenCalledWith('provider_apply', expect.anything());
  fireEvent.click(screen.getByLabelText('I reviewed the affected paths, origin and network policy'));
  fireEvent.click(apply);
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('provider_apply', { plan_id: 'plan-1', checksum: 'reviewed-sha' }));
});

it('compares the selected provider and exposes architecture queries without applying a plan', async () => {
  vi.mocked(o2Action).mockReset(); vi.mocked(o2Action).mockResolvedValue({ baseline: {}, candidate: {} });
  render(<Providers />);
  fireEvent.change(screen.getByLabelText('Provider ID'), { target: { value: 'graphify' } });
  fireEvent.change(screen.getByLabelText('Literal code query'), { target: { value: 'caller' } });
  fireEvent.change(screen.getByLabelText('Code query type'), { target: { value: 'architecture_summary' } });
  fireEvent.click(screen.getByRole('button', { name: 'Compare provider with native' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('code_compare', { provider: 'graphify', query: { text: 'caller', kind: 'architecture_summary', worktree: null } }));
  expect(o2Action).not.toHaveBeenCalledWith('provider_apply', expect.anything());
});

it('keeps the reviewed operation visible while its worker is running', async () => {
  let finish!: (value: unknown) => void;
  const id = 'e352c025-a3bd-465b-8d64-0ac307c9e414';
  vi.mocked(o2Action).mockReset();
  vi.mocked(o2Action).mockImplementation(async action => {
    if (action === 'provider_plan') return { result: { outcome: 'supported', data: { id } }, checksum: 'hash' };
    if (action === 'provider_apply') return new Promise(resolve => { finish = resolve; });
    if (action === 'provider_operation') return { id, status: 'running', detail: 'Extracting local code' };
    return { providers: [], receipts: [], plans: [] };
  });
  render(<Providers />);
  fireEvent.click(screen.getByRole('button', { name: 'Preview provider operation' }));
  fireEvent.click(await screen.findByLabelText('I reviewed the affected paths, origin and network policy'));
  fireEvent.click(screen.getByRole('button', { name: 'Apply reviewed provider operation' }));
  await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Extracting local code'));
  expect(screen.getByLabelText('Provider ID')).toBeDisabled();
  finish({ state: 'healthy' });
  await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh provider health' })).toBeEnabled());
});

it('sends a scoped symbol query through the shared provider action', async () => {
  vi.mocked(o2Action).mockReset(); vi.mocked(o2Action).mockResolvedValue({ outcome: 'supported', data: { provider_id: 'serena', hits: [] } });
  render(<Providers />);
  fireEvent.change(screen.getByLabelText('Literal code query'), { target: { value: 'Other/target' } });
  fireEvent.change(screen.getByLabelText('Code query type'), { target: { value: 'symbol_definition' } });
  fireEvent.change(screen.getByLabelText('Definition file path (required for Serena)'), { target: { value: 'src/sample.py' } });
  fireEvent.click(screen.getByRole('button', { name: 'Search code' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('code_query', { query: { text: 'Other/target', kind: 'symbol_definition', worktree: null, path: 'src/sample.py' } }));
});
