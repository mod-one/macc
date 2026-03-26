import type { ReactNode } from 'react';
import * as ToastPrimitives from '@radix-ui/react-toast';
import { AlertTriangleIcon, CheckIcon, XCircleIcon } from './icons';
import { cn } from './styles';

const TOAST_VARIANTS = {
  error: {
    icon: XCircleIcon,
    tone: 'var(--error)',
  },
  success: {
    icon: CheckIcon,
    tone: 'var(--success)',
  },
  warning: {
    icon: AlertTriangleIcon,
    tone: 'var(--warning)',
  },
} as const;

export type ToastVariant = keyof typeof TOAST_VARIANTS;

export interface ToastProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  variant?: ToastVariant;
  action?: ReactNode;
  duration?: number;
}

export function Toast({
  open,
  onOpenChange,
  title,
  description,
  variant = 'success',
  action,
  duration = 4000,
}: ToastProps) {
  const config = TOAST_VARIANTS[variant];
  const Icon = config.icon;

  return (
    <ToastPrimitives.Provider duration={duration} swipeDirection="right">
      <ToastPrimitives.Root
        className="grid w-[min(92vw,24rem)] grid-cols-[auto_1fr_auto] items-start gap-3 rounded-[var(--radius-card)] border border-white/10 bg-[var(--bg-card)] p-4 text-[var(--text-primary)] shadow-2xl data-[state=open]:animate-[slideInRight_180ms_ease-out]"
        onOpenChange={onOpenChange}
        open={open}
      >
        <div
          className="mt-0.5 rounded-full p-2"
          style={{ backgroundColor: `${config.tone}1A`, color: config.tone }}
        >
          <Icon className="h-4 w-4" />
        </div>
        <div className="space-y-1">
          <ToastPrimitives.Title className="text-sm font-semibold">{title}</ToastPrimitives.Title>
          {description && (
            <ToastPrimitives.Description className="text-sm text-[var(--text-secondary)]">
              {description}
            </ToastPrimitives.Description>
          )}
        </div>
        <div className="flex items-center gap-2">
          {action}
          <ToastPrimitives.Close
            aria-label="Close notification"
            className={cn(
              'rounded-md border border-white/10 bg-white/5 px-2 py-1 text-xs font-medium transition-colors hover:bg-white/10',
            )}
          >
            Dismiss
          </ToastPrimitives.Close>
        </div>
      </ToastPrimitives.Root>
      <ToastPrimitives.Viewport className="fixed bottom-4 right-4 z-50 flex max-w-[100vw] flex-col gap-2 outline-none" />
    </ToastPrimitives.Provider>
  );
}
