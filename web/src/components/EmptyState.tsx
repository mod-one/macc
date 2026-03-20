import type { ReactNode } from 'react';
import { SparklesIcon } from './icons';
import { cn, surfaceClassName } from './styles';

export interface EmptyStateProps {
  title: string;
  description: string;
  icon?: ReactNode;
  action?: ReactNode;
  className?: string;
}

export function EmptyState({
  title,
  description,
  icon,
  action,
  className,
}: EmptyStateProps) {
  return (
    <section
      className={cn(
        surfaceClassName,
        'flex min-h-52 flex-col items-center justify-center gap-4 px-6 py-8 text-center',
        className,
      )}
    >
      <div className="rounded-full border border-white/10 bg-white/5 p-3 text-[var(--accent)]">
        {icon ?? <SparklesIcon className="h-6 w-6" />}
      </div>
      <div className="space-y-1">
        <h2 className="text-lg font-semibold">{title}</h2>
        <p className="max-w-md text-sm text-[var(--text-secondary)]">{description}</p>
      </div>
      {action}
    </section>
  );
}
