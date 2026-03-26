import type { ReactNode } from 'react';
import { CopyIcon, LogsIcon, RefreshIcon } from './icons';
import { cn } from './styles';

export interface ErrorBannerProps {
  title?: string;
  message: string;
  code?: string;
  onRetry?: () => void;
  onCopy?: () => void;
  onOpenLogs?: () => void;
  className?: string;
}

export function ErrorBanner({
  title = 'Something went wrong',
  message,
  code,
  onRetry,
  onCopy,
  onOpenLogs,
  className,
}: ErrorBannerProps) {
  return (
    <section
      aria-live="polite"
      className={cn('rounded-[var(--radius-card)] border p-4 text-[var(--text-primary)]', className)}
      style={{
        borderColor: 'color-mix(in srgb, var(--error) 30%, transparent)',
        backgroundColor: 'color-mix(in srgb, var(--error) 10%, transparent)',
      }}
      role="alert"
    >
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="space-y-1">
          <h2 className="font-semibold text-[var(--error)]">{title}</h2>
          <p className="text-sm text-[var(--text-primary)]">{message}</p>
          {code && <p className="font-mono text-xs text-[var(--text-secondary)]">{code}</p>}
        </div>
        <div className="flex flex-wrap gap-2">
          {onRetry && (
            <ActionButton label="Retry" onClick={onRetry}>
              <RefreshIcon className="h-4 w-4" />
            </ActionButton>
          )}
          {onCopy && (
            <ActionButton label="Copy" onClick={onCopy}>
              <CopyIcon className="h-4 w-4" />
            </ActionButton>
          )}
          {onOpenLogs && (
            <ActionButton label="Open logs" onClick={onOpenLogs}>
              <LogsIcon className="h-4 w-4" />
            </ActionButton>
          )}
        </div>
      </div>
    </section>
  );
}

interface ActionButtonProps {
  label: string;
  onClick: () => void;
  children: ReactNode;
}

function ActionButton({ label, onClick, children }: ActionButtonProps) {
  return (
    <button
      className="inline-flex items-center gap-2 rounded-md border bg-black/20 px-3 py-2 text-sm font-medium text-[var(--text-primary)] transition-colors hover:bg-black/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--error)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg-primary)]"
      style={{ borderColor: 'color-mix(in srgb, var(--error) 25%, transparent)' }}
      onClick={onClick}
      type="button"
    >
      {children}
      <span>{label}</span>
    </button>
  );
}
