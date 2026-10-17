import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import Memory from './Memory';
import { o2Action, type MemoryHit } from '../../../api/o2';
vi.mock('../../../api/o2', () => ({ o2Action: vi.fn() }));
vi.mock('./AgentMemory', () => ({ default: () => null }));
const hit = { memory: { id: 'A-record', status: 'proposed', proposal: { title: 'A evidence', kind: 'discovery', scope: 'task', scope_ref: 'A', confidence: 1, evidence: [] } }, checksum: 'A-checksum' } as unknown as MemoryHit;
beforeEach(() => { vi.resetAllMocks(); vi.mocked(o2Action).mockImplementation(async action => action === 'memory_search' ? [hit] : hit); });
const search = async () => { fireEvent.click(screen.getByRole('button', { name: 'Search memory inbox' })); await screen.findByRole('button', { name: /A evidence/ }); };
const inspect = async () => { await search(); fireEvent.click(screen.getByRole('button', { name: /A evidence/ })); await screen.findByRole('region', { name: 'Memory review' }); };
it('does not repopulate old search results after a route scope change', async () => {
  let resolve!: (value: unknown) => void;
  vi.mocked(o2Action).mockImplementation(() => new Promise(r => { resolve = r; }));
  const view = render(<Memory taskScope="A" />);
  fireEvent.click(screen.getByRole('button', { name: 'Search memory inbox' }));
  view.rerender(<Memory taskScope="B" />);
  await act(async () => { resolve([hit]); });
  expect(screen.getByLabelText('Task scope')).toHaveValue('B');
  expect(screen.queryByText(/A evidence/)).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Search memory inbox' })).toBeEnabled();
});
it.each(['Task scope', 'Tool scope', 'Worktree scope', 'Feature scope', 'User scope', 'Search memories'])('clears review and pull evidence when %s changes, preserving proposal drafts', async label => {
  render(<Memory taskScope="A" />); await inspect();
  fireEvent.change(screen.getByLabelText('Memory title'), { target: { value: 'keep proposal' } });
  fireEvent.click(screen.getByLabelText('I reviewed the source evidence and this transition'));
  expect(screen.getByRole('button', { name: 'Apply reviewed memory transition' })).toBeEnabled();
  fireEvent.click(screen.getByRole('button', { name: 'Preview bounded agent record' }));
  expect(await screen.findByText(/A-checksum/)).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText(label), { target: { value: 'other-scope' } });
  expect(screen.queryByRole('region', { name: 'Memory review' })).not.toBeInTheDocument();
  expect(screen.queryByText(/A-checksum/)).not.toBeInTheDocument();
  expect(screen.getByLabelText('Memory title')).toHaveValue('keep proposal');
  await inspect();
  expect(screen.getByLabelText('I reviewed the source evidence and this transition')).not.toBeChecked();
});
it('ignores a late record selection after a task change', async () => {
  const view = render(<Memory taskScope="A" />); await search();
  let resolve!: (value: unknown) => void;
  vi.mocked(o2Action).mockImplementation(() => new Promise(r => { resolve = r; }));
  fireEvent.click(screen.getByRole('button', { name: /A evidence/ }));
  view.rerender(<Memory taskScope="B" />);
  await act(async () => { resolve(hit); });
  expect(screen.queryByRole('region', { name: 'Memory review' })).not.toBeInTheDocument();
});
it('does not submit a follow-up review write after scope changes during evidence loading', async () => {
  const view = render(<Memory taskScope="A" />); await inspect();
  fireEvent.change(screen.getByLabelText('Replacement evidence path (optional revalidation)'), { target: { value: 'contract.rs' } });
  fireEvent.click(screen.getByLabelText('I reviewed the source evidence and this transition'));
  let resolve!: (value: unknown) => void;
  vi.mocked(o2Action).mockImplementation(() => new Promise(r => { resolve = r; }));
  fireEvent.click(screen.getByRole('button', { name: 'Apply reviewed memory transition' }));
  view.rerender(<Memory taskScope="B" />);
  await act(async () => { resolve({ revision: 'A-head' }); });
  expect(vi.mocked(o2Action).mock.calls.some(([action]) => action === 'memory_review')).toBe(false);
});

it.each(['Preview bounded agent search', 'Inspect memory conflicts'])('ignores late %s results after changing scope', async name => {
  let resolve!: (value: unknown) => void;
  vi.mocked(o2Action).mockImplementation(() => new Promise(r => { resolve = r; }));
  const view = render(<Memory taskScope="A" />);
  fireEvent.click(screen.getByRole('button', { name }));
  view.rerender(<Memory taskScope="B" />);
  await act(async () => { resolve(name.includes('search') ? { id: 'obsolete-pull' } : [{ id: 'old', reason: 'obsolete-conflict', status: 'open' }]); });
  expect(screen.queryByText(/obsolete-/)).not.toBeInTheDocument();
});
