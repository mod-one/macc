import { StatusBadge, type StatusTone } from './StatusBadge';
import { cn, interactiveSurfaceClassName, surfaceClassName } from './styles';

export interface TaskListItemProps {
  taskId: string;
  title: string;
  state: string;
  stateTone?: StatusTone;
  tool: string;
  attempts: number;
  priority: string | number;
  onSelect?: () => void;
  className?: string;
}

export function TaskListItem({
  taskId,
  title,
  state,
  stateTone = 'todo',
  tool,
  attempts,
  priority,
  onSelect,
  className,
}: TaskListItemProps) {
  const content = (
    <>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <p className="font-mono text-xs uppercase tracking-wide text-[var(--text-muted)]">{taskId}</p>
          <h3 className="mt-1 text-base font-semibold">{title}</h3>
        </div>
        <StatusBadge status={state} tone={stateTone} />
      </div>
      <div className="flex flex-wrap gap-4 text-sm text-[var(--text-secondary)]">
        <span>Tool: {tool}</span>
        <span>Attempts: {attempts}</span>
        <span>Priority: {priority}</span>
      </div>
    </>
  );

  const baseClassName = cn(
    surfaceClassName,
    interactiveSurfaceClassName,
    'grid w-full gap-3 p-4 text-left',
    className,
  );

  if (onSelect) {
    return (
      <button
        className={cn(
          baseClassName,
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg-primary)]',
        )}
        onClick={onSelect}
        type="button"
      >
        {content}
      </button>
    );
  }

  return <article className={baseClassName}>{content}</article>;
}
