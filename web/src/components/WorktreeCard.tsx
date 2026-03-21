import React from 'react';
import type { ApiWorktree } from '../api/models';
import { Button } from './Button';
import { StatusBadge, type StatusTone } from './StatusBadge';
import * as Icons from './icons';
import { cn, interactiveSurfaceClassName, surfaceClassName } from './styles';

export interface WorktreeCardProps {
  worktree: ApiWorktree;
  onRun: (id: string) => void;
  onDoctor: (id: string) => void;
  onRemove: (id: string) => void;
  onOpen: (id: string) => void;
}

export const WorktreeCard: React.FC<WorktreeCardProps> = ({
  worktree,
  onRun,
  onDoctor,
  onRemove,
  onOpen,
}) => {
  const getStatusTone = (status: string | null): StatusTone => {
    switch (status?.toLowerCase()) {
      case 'active':
      case 'running':
        return 'active';
      case 'blocked':
        return 'blocked';
      case 'failed':
      case 'error':
        return 'failed';
      case 'merged':
      case 'success':
        return 'merged';
      default:
        return 'todo';
    }
  };

  return (
    <article className={cn(surfaceClassName, interactiveSurfaceClassName, 'p-4 flex flex-col gap-4')}>
      <div className="flex items-start justify-between">
        <div className="flex flex-col gap-1 overflow-hidden">
          <h3 className="font-bold text-lg truncate" title={worktree.id}>
            {worktree.slug || worktree.id}
          </h3>
          <div className="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
            <Icons.BranchIcon className="h-3.5 w-3.5" />
            <span className="truncate">{worktree.branch || 'no branch'}</span>
          </div>
        </div>
        <StatusBadge status={worktree.status || 'unknown'} tone={getStatusTone(worktree.status)} />
      </div>

      <div className="flex flex-wrap gap-2">
        {worktree.tool && (
          <span className="px-2 py-0.5 rounded-md bg-[var(--bg-secondary)] border border-[var(--border)] text-[10px] font-bold uppercase tracking-wider text-[var(--text-secondary)]">
            {worktree.tool}
          </span>
        )}
        {worktree.scope && (
          <span className="px-2 py-0.5 rounded-md bg-[var(--bg-secondary)] border border-[var(--border)] text-[10px] font-bold uppercase tracking-wider text-[var(--text-muted)]">
            {worktree.scope}
          </span>
        )}
      </div>

      <div className="mt-auto pt-4 flex items-center justify-between border-t border-[var(--border)]">
        <div className="flex items-center gap-2">
          <Button
            className="p-1 h-8 w-8 bg-transparent border-none hover:bg-white/10"
            onClick={() => onRun(worktree.id)}
            title="Run"
          >
            <Icons.PlayIcon className="h-4 w-4" />
          </Button>
          <Button
            className="p-1 h-8 w-8 bg-transparent border-none hover:bg-white/10 text-[var(--accent)]"
            onClick={() => onDoctor(worktree.id)}
            title="Doctor"
          >
            <Icons.ActivityIcon className="h-4 w-4" />
          </Button>
          <Button
            className="p-1 h-8 w-8 bg-transparent border-none hover:bg-white/10 text-rose-500 hover:text-rose-600"
            onClick={() => onRemove(worktree.id)}
            title="Remove"
          >
            <Icons.TrashIcon className="h-4 w-4" />
          </Button>
        </div>
        <Button onClick={() => onOpen(worktree.id)} className="gap-2 h-8">
          Open
          <Icons.ArrowRightIcon className="h-4 w-4" />
        </Button>
      </div>
    </article>
  );
};
