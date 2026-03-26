import { cn } from './styles';

export interface LoadingSpinnerProps {
  label?: string;
  size?: 'sm' | 'md' | 'lg';
  className?: string;
}

const SIZE_MAP = {
  sm: 'h-4 w-4 border-2',
  md: 'h-6 w-6 border-2',
  lg: 'h-10 w-10 border-[3px]',
} as const;

export function LoadingSpinner({
  label = 'Loading',
  size = 'md',
  className,
}: LoadingSpinnerProps) {
  return (
    <span className={cn('inline-flex items-center gap-2 text-sm text-[var(--text-secondary)]', className)}>
      <span
        aria-hidden="true"
        className={cn(
          'inline-block animate-spin rounded-full border-[var(--border)] border-t-[var(--accent)]',
          SIZE_MAP[size],
        )}
      />
      <span className="sr-only" role="status">
        {label}
      </span>
    </span>
  );
}
