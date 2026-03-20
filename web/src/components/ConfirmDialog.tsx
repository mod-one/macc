import { useState } from 'react';
import * as AlertDialog from '@radix-ui/react-alert-dialog';
import { AlertTriangleIcon } from './icons';
import { cn } from './styles';

export type ConfirmDialogIntent = 'caution' | 'danger';
export type DangerousConfirmationMode = 'phrase' | 'double-confirm';

export interface ConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: string;
  confirmLabel?: string;
  cancelLabel?: string;
  intent?: ConfirmDialogIntent;
  confirmationPhrase?: string;
  dangerousConfirmationMode?: DangerousConfirmationMode;
  secondaryConfirmationLabel?: string;
  onConfirm: () => void;
}

export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel = 'Confirm',
  cancelLabel = 'Cancel',
  intent = 'caution',
  confirmationPhrase = 'CONFIRM',
  dangerousConfirmationMode = 'phrase',
  secondaryConfirmationLabel = 'I understand this action cannot be undone.',
  onConfirm,
}: ConfirmDialogProps) {
  const [typedPhrase, setTypedPhrase] = useState('');
  const [isAcknowledged, setIsAcknowledged] = useState(false);
  const isDangerous = intent === 'danger';
  const requiresPhrase = isDangerous && dangerousConfirmationMode === 'phrase';
  const isConfirmed = !isDangerous || (requiresPhrase ? typedPhrase.trim() === confirmationPhrase : isAcknowledged);

  return (
    <AlertDialog.Root
      onOpenChange={(nextOpen) => {
        if (!nextOpen) {
          setTypedPhrase('');
          setIsAcknowledged(false);
        }
        onOpenChange(nextOpen);
      }}
      open={open}
    >
      <AlertDialog.Portal>
        <AlertDialog.Overlay className="fixed inset-0 bg-black/70 backdrop-blur-sm" />
        <AlertDialog.Content className="fixed left-1/2 top-1/2 w-[min(92vw,32rem)] -translate-x-1/2 -translate-y-1/2 rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-secondary)] p-6 text-[var(--text-primary)] shadow-2xl focus:outline-none">
          <div className="flex items-start gap-3">
            <div
              className={cn('rounded-full p-2', isDangerous ? 'text-[var(--error)]' : 'text-[var(--warning)]')}
              style={{
                backgroundColor: isDangerous
                  ? 'color-mix(in srgb, var(--error) 15%, transparent)'
                  : 'color-mix(in srgb, var(--warning) 15%, transparent)',
              }}
            >
              <AlertTriangleIcon className="h-5 w-5" />
            </div>
            <div className="space-y-2">
              <AlertDialog.Title className="text-lg font-semibold">{title}</AlertDialog.Title>
              <AlertDialog.Description className="text-sm text-[var(--text-secondary)]">
                {description}
              </AlertDialog.Description>
            </div>
          </div>

          {requiresPhrase && (
            <div className="mt-5 space-y-2">
              <label className="block text-sm font-medium" htmlFor="danger-confirmation-phrase">
                Type <span className="font-mono text-[var(--text-primary)]">{confirmationPhrase}</span> to
                continue
              </label>
              <input
                className="w-full rounded-md border border-white/10 bg-black/30 px-3 py-2 text-sm text-[var(--text-primary)] outline-none transition focus:border-[var(--accent)]"
                id="danger-confirmation-phrase"
                onChange={(event) => setTypedPhrase(event.target.value)}
                value={typedPhrase}
              />
            </div>
          )}

          {isDangerous && !requiresPhrase && (
            <div className="mt-5">
              <label className="flex items-start gap-3 rounded-md border border-white/10 bg-black/20 px-3 py-3 text-sm text-[var(--text-secondary)]">
                <input
                  checked={isAcknowledged}
                  className="mt-0.5 h-4 w-4 rounded border-white/20 bg-transparent text-[var(--accent)]"
                  onChange={(event) => setIsAcknowledged(event.target.checked)}
                  type="checkbox"
                />
                <span>{secondaryConfirmationLabel}</span>
              </label>
            </div>
          )}

          <div className="mt-6 flex justify-end gap-3">
            <AlertDialog.Cancel asChild>
              <button
                className="rounded-md border border-white/10 bg-white/5 px-4 py-2 text-sm font-medium text-[var(--text-primary)] transition-colors hover:bg-white/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg-secondary)]"
                type="button"
              >
                {cancelLabel}
              </button>
            </AlertDialog.Cancel>
            <AlertDialog.Action asChild>
              <button
                className="rounded-md bg-[var(--accent)] px-4 py-2 text-sm font-semibold text-white transition-opacity focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg-secondary)] disabled:cursor-not-allowed disabled:opacity-50"
                disabled={!isConfirmed}
                onClick={onConfirm}
                type="button"
              >
                {confirmLabel}
              </button>
            </AlertDialog.Action>
          </div>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}
