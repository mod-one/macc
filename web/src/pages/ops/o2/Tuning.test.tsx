import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import { o2Action } from '../../../api/o2';
import Tuning from './Tuning';
vi.mock('../../../api/o2', () => ({ o2Action: vi.fn() }));
beforeEach(() => vi.resetAllMocks());
const report = { checksum: 'report-sha', report: { id: 'result-A', passed: true } };
const trial = { id: 'trial-A', plan: { cases: [{ task_id: 'TASK-A', revision: 'a'.repeat(40) }] } };
const confirm = () => fireEvent.click(screen.getByLabelText('I reviewed this exact operation, controlled trial conditions and evidence'));
async function inspect(value: unknown) {
  vi.mocked(o2Action).mockResolvedValue(value);
  fireEvent.click(screen.getByRole('button', { name: 'Inspect policy trials' }));
  await screen.findByRole('status');
}
it('prepares a typed pre-registered candidate and requires separate exact confirmation', async () => {
  render(<Tuning />); await inspect({ fields: ['memory.ranking_task_weight'], trials: [] });
  fireEvent.change(screen.getByLabelText('Candidate numeric value'), { target: { value: '60' } });
  fireEvent.change(screen.getByLabelText('Representative task IDs'), { target: { value: 'A, B' } });
  fireEvent.change(screen.getByLabelText('Full starting Git revision'), { target: { value: 'a'.repeat(40) } });
  fireEvent.change(screen.getByLabelText('Trial protocol file'), { target: { value: 'trial.md' } });
  fireEvent.change(screen.getByLabelText('Protocol SHA-256'), { target: { value: 'protocol-sha' } });
  fireEvent.click(screen.getByRole('button', { name: 'Prepare trial registration' }));
  expect(o2Action).toHaveBeenCalledTimes(1);
  const apply = screen.getByRole('button', { name: 'Confirm prepared trial operation' }); expect(apply).toBeDisabled(); confirm(); fireEvent.click(apply);
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('tuning_create', { confirmed: true, plan: { field: 'memory.ranking_task_weight', candidate: 60, cases: ['A', 'B'].map(task_id => ({ task_id, revision: 'a'.repeat(40) })), protocol_path: 'trial.md', protocol_sha256: 'protocol-sha' } }));
  expect(o2Action).not.toHaveBeenCalledWith('tuning_apply', expect.anything());
});
it('adopts only a passing exact report after revision-bound preview and explicit confirmation', async () => {
  render(<Tuning />); await inspect({ trial, result: report });
  vi.mocked(o2Action).mockResolvedValue({ report_id: 'result-A', checksum: 'report-sha', revision: 'config-revision', before: { field: 100 }, after: { field: 60 } });
  fireEvent.click(screen.getByRole('button', { name: 'Preview passing policy recommendation' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Prepare reviewed policy adoption' })); confirm();
  fireEvent.click(screen.getByRole('button', { name: 'Confirm prepared trial operation' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('tuning_apply', { report_id: 'result-A', checksum: 'report-sha', revision: 'config-revision', confirmed: true }));
});
it('keeps failed trials inspectable without enabling adoption', async () => {
  render(<Tuning />); await inspect({ trial, result: { ...report, report: { ...report.report, passed: false } } });
  expect(screen.getByRole('button', { name: 'Preview passing policy recommendation' })).toBeDisabled();
  expect(screen.queryByRole('button', { name: 'Prepare reviewed policy adoption' })).not.toBeInTheDocument();
});
it('collects paired quality and evidence while costs stay canonical', async () => {
  render(<Tuning />); fireEvent.change(screen.getByLabelText('Trial ID'), { target: { value: 'trial-A' } }); await inspect({ trial });
  fireEvent.change(screen.getByLabelText('Registered task'), { target: { value: 'TASK-A' } });
  for (const arm of ['Baseline', 'Candidate']) for (const flag of ['accepted', 'tests passed', 'spec fidelity']) fireEvent.click(screen.getByLabelText(`${arm} ${flag}`));
  fireEvent.change(screen.getByLabelText('Task quality evidence file'), { target: { value: 'results.md' } });
  fireEvent.change(screen.getByLabelText('Quality evidence SHA-256'), { target: { value: 'evidence-sha' } });
  fireEvent.click(screen.getByRole('button', { name: 'Prepare sealed quality observation' })); confirm();
  fireEvent.click(screen.getByRole('button', { name: 'Confirm prepared trial operation' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('tuning_observe', { trial_id: 'trial-A', confirmed: true, observation: { task_id: 'TASK-A', baseline: { accepted: true, tests_passed: true, spec_fidelity: true, max_finding_severity: 0, retries: 0 }, candidate: { accepted: true, tests_passed: true, spec_fidelity: true, max_finding_severity: 0, retries: 0 }, evidence_path: 'results.md', evidence_sha256: 'evidence-sha' } }));
});
it('clears prepared mutations when a task or trial changes', async () => {
  render(<Tuning />); await inspect({ trial });
  fireEvent.click(screen.getByRole('button', { name: 'Prepare trial cancellation' })); confirm();
  fireEvent.change(screen.getByLabelText('Trial ID'), { target: { value: 'trial-B' } });
  expect(screen.queryByRole('button', { name: 'Confirm prepared trial operation' })).not.toBeInTheDocument();
  expect(o2Action).not.toHaveBeenCalledWith('tuning_close', expect.anything());
});
it('discards late inspection from another selected trial', async () => {
  let finish!: (value: unknown) => void; vi.mocked(o2Action).mockImplementation(() => new Promise(resolve => { finish = resolve; }));
  render(<Tuning />); fireEvent.click(screen.getByRole('button', { name: 'Inspect policy trials' }));
  fireEvent.change(screen.getByLabelText('Trial ID'), { target: { value: 'trial-B' } });
  await act(async () => finish({ trial, result: report }));
  expect(screen.queryByRole('button', { name: 'Preview passing policy recommendation' })).not.toBeInTheDocument();
});
it('uses canonical numeric bounds and expires confirmation after configuration changes', async () => {
  const view = { project_root: '/repo', revision: 'v1', source: 'project', sqlite: true, ready: true, policy: { memory: { ranking_task_weight: 100 } }, fields: [{ path: 'memory.ranking_task_weight', label: 'Weight', kind: 'number' as const, min: 0, max: 1000, default: 100, choices: [], activation: 'next query' }] };
  const page = render(<Tuning view={view} />); await inspect({ fields: ['memory.ranking_task_weight'] });
  for (const [label, value] of [['Candidate numeric value', '1001'], ['Representative task IDs', 'A,B'], ['Full starting Git revision', 'a'.repeat(40)], ['Trial protocol file', 'protocol.md'], ['Protocol SHA-256', 'sha']]) fireEvent.change(screen.getByLabelText(label), { target: { value } });
  expect(screen.getByRole('button', { name: 'Prepare trial registration' })).toBeDisabled();
  expect(screen.getByLabelText('Candidate numeric value')).toHaveAttribute('max', '1000');
  fireEvent.change(screen.getByLabelText('Candidate numeric value'), { target: { value: '60' } });
  fireEvent.click(screen.getByRole('button', { name: 'Prepare trial registration' })); confirm();
  page.rerender(<Tuning view={{ ...view, revision: 'v2' }} />);
  expect(screen.queryByRole('button', { name: 'Confirm prepared trial operation' })).not.toBeInTheDocument();
});
