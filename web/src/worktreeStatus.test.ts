import { describe, it, expect } from 'vitest';
import { isRunningWorktree, worktreeStatusLabel, worktreeStatusTone } from './worktreeStatus';
describe('worktree activity', () => {
  it('counts running performers independently from retained and supporting workspaces', () => {
    const states=['in_progress','blocked','retained','waiting','supervisor','integration','idle'];
    expect(states.filter(isRunningWorktree)).toEqual(['in_progress']);
    expect(worktreeStatusLabel('blocked')).toContain('retained');
    expect(worktreeStatusLabel('waiting')).toContain('no live performer');
    expect(worktreeStatusTone('in_progress')).toBe('active');
    expect(worktreeStatusTone('retained')).toBe('blocked');
  });
});
