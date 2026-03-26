import type { ReactNode } from 'react';
import { ArrowDownIcon, ArrowUpIcon, MinusIcon } from './icons';
import { cn, interactiveSurfaceClassName, surfaceClassName } from './styles';

export interface KpiCardProps {
  title: string;
  value: string | number;
  delta?: number;
  deltaLabel?: string;
  icon?: ReactNode;
  description?: string;
  className?: string;
}

export function KpiCard({
  title,
  value,
  delta,
  deltaLabel,
  icon,
  description,
  className,
}: KpiCardProps) {
  const DeltaIcon = delta === undefined ? MinusIcon : delta >= 0 ? ArrowUpIcon : ArrowDownIcon;
  const deltaTone =
    delta === undefined ? 'var(--text-muted)' : delta >= 0 ? 'var(--success)' : 'var(--error)';
  const formattedDelta =
    delta === undefined ? 'No change' : `${delta > 0 ? '+' : ''}${delta.toFixed(1)}%`;

  return (
    <article className={cn(surfaceClassName, interactiveSurfaceClassName, 'p-5', className)}>
      <div className="flex items-start justify-between gap-4">
        <div className="space-y-2">
          <p className="text-sm font-medium text-[var(--text-secondary)]">{title}</p>
          <p className="text-3xl font-semibold tracking-tight">{value}</p>
          {(deltaLabel || delta !== undefined) && (
            <p className="inline-flex items-center gap-1.5 text-sm" style={{ color: deltaTone }}>
              <DeltaIcon className="h-4 w-4" />
              <span>{deltaLabel ?? formattedDelta}</span>
            </p>
          )}
        </div>
        {icon && (
          <div className="rounded-xl border border-white/10 bg-white/5 p-3 text-[var(--accent)]">
            {icon}
          </div>
        )}
      </div>
      {description && <p className="mt-4 text-sm text-[var(--text-secondary)]">{description}</p>}
    </article>
  );
}
