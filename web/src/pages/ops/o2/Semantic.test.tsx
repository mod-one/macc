import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import Semantic from './Semantic';
import { o2Action } from '../../../api/o2';
vi.mock('../../../api/o2', () => ({ o2Action: vi.fn() }));
beforeEach(() => vi.resetAllMocks());
const access = { task_id: 'A', tool_id: null, worktree: null, feature: null, user_id: null };
const report = { checksum: 'reviewed-report-sha', report: { id: 'report-A', passed: true, fts_recall: 0.3, hybrid_recall: 1 } };
it('requires inspection and confirmation of a passing benchmark before approving its exact checksum', async () => {
  vi.mocked(o2Action).mockImplementation(async action => action === 'semantic_status' ? { reports: [report] } : { enabled: true });
  render(<Semantic identity="A" access={access} />);
  fireEvent.click(screen.getByRole('button', { name: 'Inspect semantic benchmark status' }));
  const approve = await screen.findByRole('button', { name: 'Approve passing semantic benchmark' }); expect(approve).toBeDisabled();
  fireEvent.click(screen.getByLabelText('I reviewed the exact corpus, representative labels, quality and resource measurements'));
  fireEvent.click(approve);
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('semantic_gate', { report_id: 'report-A', checksum: 'reviewed-report-sha', enabled: true, confirmed: true }));
});
it('never enables approval for a rejected report and supports explicit revocation', async () => {
  vi.mocked(o2Action).mockResolvedValue({ reports: [{ ...report, report: { ...report.report, passed: false } }] });
  render(<Semantic identity="A" access={access} />); fireEvent.click(screen.getByRole('button', { name: 'Inspect semantic benchmark status' }));
  fireEvent.click(await screen.findByLabelText('I reviewed the exact corpus, representative labels, quality and resource measurements'));
  expect(screen.getByRole('button', { name: 'Approve passing semantic benchmark' })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: 'Revoke semantic gate for this corpus' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('semantic_gate', { report_id: 'report-A', checksum: 'reviewed-report-sha', enabled: false, confirmed: true }));
});
it('ignores late reports from a previous task scope', async () => {
  let finish!: (value: unknown) => void;
  vi.mocked(o2Action).mockImplementation(() => new Promise(resolve => { finish = resolve; }));
  const page = render(<Semantic identity="A" access={access} />); fireEvent.click(screen.getByRole('button', { name: 'Inspect semantic benchmark status' }));
  page.rerender(<Semantic identity="B" access={{ ...access, task_id: 'B' }} />);
  await act(async () => finish({ reports: [report] }));
  expect(screen.queryByRole('button', { name: 'Approve passing semantic benchmark' })).not.toBeInTheDocument();
});
it('submits labelled typed queries without automatically approving the result', async () => {
  vi.mocked(o2Action).mockResolvedValue(report); render(<Semantic identity="A" access={access} />);
  for (let i = 1; i <= 4; i++) { fireEvent.change(screen.getByLabelText(`Benchmark query ${i}`), { target: { value: `query ${i}` } }); fireEvent.change(screen.getByLabelText(`Relevant memory IDs ${i}`), { target: { value: `memory-${i}` } }); }
  fireEvent.click(screen.getByRole('button', { name: 'Run labelled local benchmark' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('semantic_benchmark', { suite: { access, top_k: 3, cases: [1, 2, 3, 4].map(i => ({ query: `query ${i}`, relevant_ids: [`memory-${i}`] })) } }));
  expect(o2Action).not.toHaveBeenCalledWith('semantic_gate', expect.anything());
});
it('queries only approved facts through the existing scoped search action', async () => {
  vi.mocked(o2Action).mockResolvedValue([]); render(<Semantic identity="A" access={access} />);
  fireEvent.change(screen.getByLabelText('Approved semantic query'), { target: { value: 'sign in' } });
  fireEvent.click(screen.getByRole('button', { name: 'Search approved facts with local semantics' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('memory_search', { search: { query: 'sign in', access, include_inactive: false } }));
});
