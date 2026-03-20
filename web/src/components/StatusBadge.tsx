import type { ReactNode } from 'react';
import { AlertTriangleIcon, CheckIcon, ClockIcon, MinusIcon, XCircleIcon } from './icons';
import { cn } from './styles';

const STATUS_STYLES = {
  active: {
    icon: CheckIcon,
    accent: 'var(--status-active)',
    label: 'Active',
  },
  blocked: {
    icon: AlertTriangleIcon,
    accent: 'var(--status-blocked)',
    label: 'Blocked',
  },
  failed: {
    icon: XCircleIcon,
    accent: 'var(--status-failed)',
    label: 'Failed',
  },
  merged: {
    icon: CheckIcon,
    accent: 'var(--status-merged)',
    label: 'Merged',
  },
  paused: {
    icon: ClockIcon,
    accent: 'var(--status-paused)',
    label: 'Paused',
  },
  todo: {
    icon: MinusIcon,
    accent: 'var(--status-todo)',
    label: 'Todo',
  },
} as const;

export type StatusTone = keyof typeof STATUS_STYLES;

export interface StatusBadgeProps {
  status: string;
  tone?: StatusTone;
  icon?: ReactNode;
  className?: string;
}

export function StatusBadge({ status, tone = 'todo', icon, className }: StatusBadgeProps) {
  const config = STATUS_STYLES[tone];
  const Icon = config.icon;

  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-semibold tracking-wide uppercase',
        className,
      )}
      style={{
        borderColor: `${config.accent}55`,
        backgroundColor: `${config.accent}1A`,
        color: config.accent,
      }}
    >
      {icon ?? <Icon className="h-3.5 w-3.5" />}
      <span>{status || config.label}</span>
    </span>
  );
}
