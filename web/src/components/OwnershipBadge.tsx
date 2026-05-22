import {
  useOwnershipPolling,
  useOwnershipRecord,
  useOwnershipStatus,
} from '../stores/ownershipStore';
import { RequestTakeoverButton } from './RequestTakeoverButton';
import { cn } from './styles';

export interface OwnershipBadgeProps {
  className?: string;
}

export function OwnershipBadge({ className }: OwnershipBadgeProps) {
  useOwnershipPolling();

  const status = useOwnershipStatus();
  const record = useOwnershipRecord();

  const ownerShortId = record?.owner?.client_id.slice(0, 8) ?? '—';
  const viewerCount = record?.viewers.length ?? 0;

  const badgeClass = cn(
    'inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-semibold',
    status === 'owner' && 'bg-emerald-500/15 text-emerald-400',
    status === 'viewer' && 'bg-amber-500/15 text-amber-400',
    status === 'unregistered' && 'bg-[var(--bg-secondary)] text-[var(--text-muted)]',
    className,
  );

  return (
    <div className="flex items-center gap-2">
      <span className={badgeClass}>
        <span
          className={cn(
            'h-1.5 w-1.5 rounded-full',
            status === 'owner' && 'bg-emerald-400',
            status === 'viewer' && 'bg-amber-400',
            status === 'unregistered' && 'bg-[var(--text-muted)]',
          )}
        />
        {status === 'owner' && 'Owner'}
        {status === 'viewer' && 'Viewer'}
        {status === 'unregistered' && 'Unknown'}
        <span className="opacity-70">·</span>
        <span className="font-mono opacity-70">{ownerShortId}</span>
        {viewerCount > 0 && (
          <span className="opacity-70">· {viewerCount}v</span>
        )}
      </span>
      {status === 'viewer' && <RequestTakeoverButton />}
    </div>
  );
}
