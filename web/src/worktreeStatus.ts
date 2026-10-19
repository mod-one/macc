import type { StatusTone } from './components/StatusBadge';
export const isRunningWorktree = (status?: string | null) => ['active', 'running', 'in_progress'].includes(status ?? '');
export function worktreeStatusLabel(status?: string | null): string {
  const labels: Record<string, string> = {
    in_progress: 'Running', blocked: 'Blocked · slot retained', waiting: 'Waiting · no live performer',
    retained: 'Uncommitted work retained', idle: 'Idle', supervisor: 'Supervisor workspace', integration: 'Merge workspace',
  };
  return labels[status ?? ''] ?? status ?? 'Unknown';
}
export function worktreeStatusTone(status?: string | null): StatusTone {
  if (isRunningWorktree(status)) return 'active';
  if (['blocked', 'retained', 'waiting'].includes(status ?? '')) return 'blocked';
  if (['failed','error'].includes(status ?? '')) return 'failed';
  if (['merged','success'].includes(status ?? '')) return 'merged';
  return 'todo';
}
