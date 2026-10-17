import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import axe from 'axe-core';
import { beforeEach, expect, it, vi } from 'vitest';
import { o2Action } from '../../../api/o2';
import Semantic from './Semantic';
import Tuning from './Tuning';

vi.mock('../../../api/o2', () => ({ o2Action: vi.fn() }));
beforeEach(() => vi.resetAllMocks());

it('offers a keyboard-reviewed semantic approval and labels every benchmark input', async () => {
  const user = userEvent.setup();
  vi.mocked(o2Action).mockResolvedValue({ reports: [{ checksum: 'sha', approvable: true, report: { id: 'current', passed: true } }] });
  render(<main><Semantic identity="project" access={{}} /></main>);
  const details = screen.getByText('Local semantic search and benchmark gate');
  fireEvent.click(details);
  const inspect = screen.getByRole('button', { name: 'Inspect semantic benchmark status' });
  inspect.focus();
  await user.keyboard('{Enter}');
  const confirm = await screen.findByLabelText('I reviewed the exact corpus, representative labels, quality and resource measurements');
  confirm.focus();
  await user.keyboard(' ');
  const approve = screen.getByRole('button', { name: 'Approve passing semantic benchmark' });
  expect(approve).toBeEnabled();
  expect(o2Action).toHaveBeenCalledTimes(1);
  const result = await axe.run(document.body, { rules: { 'color-contrast': { enabled: false } } });
  expect(result.violations).toEqual([]);
});

it('labels supervised trial quality controls and explicit evaluation confirmation', async () => {
  vi.mocked(o2Action).mockResolvedValue({ trial: { id: 'trial', plan: { cases: [{ task_id: 'task' }] } } });
  render(<main><Tuning /></main>);
  fireEvent.change(screen.getByLabelText('Trial ID'), { target: { value: 'trial' } });
  fireEvent.click(screen.getByRole('button', { name: 'Inspect policy trials' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Prepare fixed-sample evaluation' }));
  expect(screen.getByRole('button', { name: 'Confirm prepared trial operation' })).toBeDisabled();
  const result = await axe.run(document.body, { rules: { 'color-contrast': { enabled: false } } });
  expect(result.violations).toEqual([]);
});
