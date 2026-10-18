import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { CoordinatorLaunchDialog } from './CoordinatorLaunchDialog';
import { getCoordinatorLaunchReview } from '../api/client';
import type { CoordinatorLaunchReview } from '../api/models';
import { closeCoordinatorLaunch, requestCoordinatorLaunch } from '../stores/coordinatorLaunchStore';
vi.mock('../api/client', () => ({ getCoordinatorLaunchReview: vi.fn() }));
const review: CoordinatorLaunchReview = {
  revision: 'review-1', project: '/project', taskSource: 'second.json (2/2)', queue: ['first.json','second.json'],
  referenceBranch: 'main', tools: ['future-tool'], maxParallel: 2, maxDispatch: 4,
  needsBranchCreation: false, branchSources: ['HEAD','origin/main'], dirtyReference: false, warnings: [], blockers: [],
};
beforeEach(() => { vi.mocked(getCoordinatorLaunchReview).mockResolvedValue(review); });
afterEach(() => { closeCoordinatorLaunch(); vi.clearAllMocks(); });
it('returns explicit choices only after confirmation', async () => {
  const decision = requestCoordinatorLaunch(); render(<CoordinatorLaunchDialog />);
  expect(await screen.findByText('second.json (2/2)')).toBeInTheDocument();
  fireEvent.click(screen.getByRole('checkbox', {name:/start supervisor/i}));
  fireEvent.click(screen.getByRole('button', {name:'Start coordinator'}));
  await expect(decision).resolves.toEqual({ confirmed:true, revision:'review-1', supervisor:false, createFrom:null, allowDirtyReference:false });
});
it('requires branch and dirty-reference decisions when applicable', async () => {
  vi.mocked(getCoordinatorLaunchReview).mockResolvedValue({...review,needsBranchCreation:true,dirtyReference:true});
  const decision=requestCoordinatorLaunch();render(<CoordinatorLaunchDialog />);
  await screen.findByRole('combobox',{name:'Reference branch source'});
  const button=screen.getByRole('button',{name:'Start coordinator'});
  expect(button).toBeDisabled();
  fireEvent.change(screen.getByRole('combobox'),{target:{value:'origin/main'}});
  expect(button).toBeDisabled();
  fireEvent.click(screen.getByRole('checkbox',{name:/allow this dirty reference once/i}));
  expect(button).toBeEnabled();fireEvent.click(button);
  await expect(decision).resolves.toMatchObject({createFrom:'origin/main',allowDirtyReference:true});
});
it('cancels without producing a launch decision and displays blockers', async () => {
  vi.mocked(getCoordinatorLaunchReview).mockResolvedValue({...review,blockers:['PRD queue changed; reset progress.']});
  const decision=requestCoordinatorLaunch(); const rejected=expect(decision).rejects.toMatchObject({name:'AbortError'});
  render(<CoordinatorLaunchDialog />);
  expect(await screen.findByRole('alert')).toHaveTextContent('PRD queue changed');
  expect(screen.getByRole('button',{name:'Start coordinator'})).toBeDisabled();
  fireEvent.click(screen.getByRole('button',{name:'Cancel'}));await rejected;
});
