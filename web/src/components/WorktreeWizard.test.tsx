import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { WorktreeWizard } from './WorktreeWizard';

vi.mock('../api/client', () => ({
  getConfig: vi.fn(async () => ({ enabledTools: ['codex', 'claude'] })),
  getGitGraph: vi.fn(async () => ({ commits: [], branches: ['main', 'develop'], head: 'main' })),
  createWorktree: vi.fn(async () => ([
    {
      id: 'demo-123',
      path: '/repo/.macc/worktree/demo-123',
      branch: 'ai/codex/demo-123',
    },
  ])),
}));

describe('WorktreeWizard', () => {
  it('walks through steps and submits create request', async () => {
    const user = userEvent.setup();
    const onComplete = vi.fn(async () => undefined);
    const onOpenChange = vi.fn();

    render(
      <WorktreeWizard
        open
        onComplete={onComplete}
        onOpenChange={onOpenChange}
        worktrees={[]}
      />,
    );

    await screen.findByLabelText('Slug *');
    await user.type(screen.getByLabelText('Slug *'), 'feature-wizard');
    await user.clear(screen.getByLabelText('Count *'));
    await user.type(screen.getByLabelText('Count *'), '2');
    await user.click(screen.getByRole('button', { name: 'Next' }));

    await user.clear(screen.getByLabelText('Base branch *'));
    await user.type(screen.getByLabelText('Base branch *'), 'develop');
    await user.type(screen.getByLabelText('Scope (CSV, optional)'), 'web/src/components');
    await user.click(screen.getByRole('button', { name: 'Next' }));

    expect(screen.getByText('Paths and branches to be created')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Create' }));

    await screen.findByText('Worktree creation complete');
    await user.click(screen.getByRole('button', { name: 'Done' }));

    await waitFor(() => expect(onComplete).toHaveBeenCalledTimes(1));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
