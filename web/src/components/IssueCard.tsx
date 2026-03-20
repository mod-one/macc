import type { ReactNode } from 'react';
import { StatusBadge, type StatusTone } from './StatusBadge';
import { cn, surfaceClassName } from './styles';

export interface IssueCardAction {
  label: string;
  onClick?: () => void;
  icon?: ReactNode;
  disabled?: boolean;
}

export interface IssueCardProps {
  severity: string;
  severityTone?: StatusTone;
  code: string;
  title: string;
  currentState: string;
  expectedState: string;
  summary?: string;
  actions?: IssueCardAction[];
  className?: string;
}

export function IssueCard({
  severity,
  severityTone = 'blocked',
  code,
  title,
  currentState,
  expectedState,
  summary,
  actions = [],
  className,
}: IssueCardProps) {
  return (
    <article className={cn(surfaceClassName, 'space-y-4 p-5', className)}>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-2">
          <StatusBadge status={severity} tone={severityTone} />
          <div>
            <h3 className="text-lg font-semibold">{title}</h3>
            <p className="font-mono text-xs text-[var(--text-muted)]">{code}</p>
          </div>
        </div>
        {actions.length > 0 && (
          <div className="flex flex-wrap gap-2">
            {actions.map((action) => (
              <button
                key={action.label}
                className="inline-flex items-center gap-2 rounded-md border border-white/10 bg-white/5 px-3 py-2 text-sm font-medium text-[var(--text-primary)] transition-colors hover:bg-white/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg-card)] disabled:cursor-not-allowed disabled:opacity-50"
                disabled={action.disabled}
                onClick={action.onClick}
                type="button"
              >
                {action.icon}
                <span>{action.label}</span>
              </button>
            ))}
          </div>
        )}
      </div>
      {summary && <p className="text-sm text-[var(--text-secondary)]">{summary}</p>}
      <dl className="grid gap-4 md:grid-cols-2">
        <div className="rounded-lg border border-white/8 bg-black/20 p-4">
          <dt className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)]">
            Current state
          </dt>
          <dd className="mt-2 text-sm text-[var(--text-primary)]">{currentState}</dd>
        </div>
        <div className="rounded-lg border border-white/8 bg-black/20 p-4">
          <dt className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)]">
            Expected state
          </dt>
          <dd className="mt-2 text-sm text-[var(--text-primary)]">{expectedState}</dd>
        </div>
      </dl>
    </article>
  );
}
