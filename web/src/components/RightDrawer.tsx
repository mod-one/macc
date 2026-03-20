import type { ReactNode } from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import { PinIcon, XCircleIcon } from './icons';
import { cn } from './styles';

export interface RightDrawerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  pinned?: boolean;
  onPinnedChange?: (pinned: boolean) => void;
  children: ReactNode;
  footer?: ReactNode;
  widthClassName?: string;
}

export function RightDrawer({
  open,
  onOpenChange,
  title,
  description,
  pinned = false,
  onPinnedChange,
  children,
  footer,
  widthClassName = 'w-full max-w-xl',
}: RightDrawerProps) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/60 backdrop-blur-sm data-[state=open]:animate-[fadeIn_150ms_ease-out]" />
        <Dialog.Content
          className={cn(
            'fixed right-0 top-0 flex h-full flex-col border-l border-[var(--border)] bg-[var(--bg-secondary)] text-[var(--text-primary)] shadow-2xl focus:outline-none data-[state=open]:animate-[slideInRight_180ms_ease-out]',
            widthClassName,
          )}
        >
          <header className="flex items-start justify-between gap-3 border-b border-white/8 px-5 py-4">
            <div className="space-y-1">
              <Dialog.Title className="text-lg font-semibold">{title}</Dialog.Title>
              {description && (
                <Dialog.Description className="text-sm text-[var(--text-secondary)]">
                  {description}
                </Dialog.Description>
              )}
            </div>
            <div className="flex items-center gap-2">
              {onPinnedChange && (
                <button
                  aria-label={pinned ? 'Unpin drawer' : 'Pin drawer'}
                  className={cn(
                    'inline-flex h-10 w-10 items-center justify-center rounded-md border transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg-secondary)]',
                    pinned
                      ? 'border-[var(--accent)] text-[var(--accent)]'
                      : 'border-white/10 bg-white/5 hover:bg-white/10',
                  )}
                  style={pinned ? { backgroundColor: 'color-mix(in srgb, var(--accent) 15%, transparent)' } : undefined}
                  onClick={() => onPinnedChange(!pinned)}
                  type="button"
                >
                  <PinIcon className="h-4 w-4" />
                </button>
              )}
              <Dialog.Close asChild>
                <button
                  aria-label="Close drawer"
                  className="inline-flex h-10 w-10 items-center justify-center rounded-md border border-white/10 bg-white/5 transition-colors hover:bg-white/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg-secondary)]"
                  type="button"
                >
                  <XCircleIcon className="h-4 w-4" />
                </button>
              </Dialog.Close>
            </div>
          </header>
          <div className="flex-1 overflow-auto px-5 py-4">{children}</div>
          {footer && <footer className="border-t border-white/8 px-5 py-4">{footer}</footer>}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
