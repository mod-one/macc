import React, { useEffect, useRef, useState } from 'react';
import { respondProjectTakeover } from '../api/client';
import { useEventSource } from '../hooks/useEventSource';
import { useIsOwner } from '../stores/ownershipStore';
import { Toast } from './Toast';
import { Button } from './Button';

interface PendingTakeoverRequest {
  requestId: string;
  requesterClientId: string;
  requestedAt: string;
}

export function TakeoverNotificationToast() {
  const isOwner = useIsOwner();
  const { events } = useEventSource('/events', { maxEvents: 20 });
  const [pending, setPending] = useState<PendingTakeoverRequest | null>(null);
  const [open, setOpen] = useState(false);
  const [isResponding, setIsResponding] = useState(false);
  const processedEventIds = useRef(new Set<string>());

  useEffect(() => {
    if (!isOwner) return;
    for (const entry of events) {
      const p = entry.payload;
      if (p.type !== 'takeover_requested') continue;
      const eventId = p.event_id;
      if (processedEventIds.current.has(eventId)) continue;
      processedEventIds.current.add(eventId);
      if (typeof p.request_id === 'string') {
        const requesterClientId =
          typeof p.requester_client_id === 'string'
            ? p.requester_client_id
            : typeof p.requester === 'object' &&
                p.requester !== null &&
                typeof (p.requester as Record<string, unknown>).client_id === 'string'
              ? ((p.requester as Record<string, unknown>).client_id as string)
              : 'unknown';
        setPending({ requestId: p.request_id, requesterClientId, requestedAt: p.ts });
        setOpen(true);
      }
    }
  }, [events, isOwner]);

  const respond = async (accept: boolean) => {
    if (!pending) return;
    setIsResponding(true);
    try {
      await respondProjectTakeover(pending.requestId, accept);
    } catch {
      // Silently ignore network errors; ownership state will sync on next poll
    } finally {
      setIsResponding(false);
      setOpen(false);
      setPending(null);
    }
  };

  if (!isOwner || !pending) return null;

  const requesterShort = pending.requesterClientId.slice(0, 8);

  return (
    <Toast
      open={open}
      onOpenChange={setOpen}
      title="Takeover request"
      description={`Client ${requesterShort} is requesting control of this process.`}
      variant="warning"
      duration={60_000}
      action={
        <div className="flex gap-1.5">
          <Button
            className="h-7 px-2 py-0 text-xs border-emerald-500/30 bg-emerald-500/10 text-emerald-400 hover:bg-emerald-500/20"
            disabled={isResponding}
            onClick={() => void respond(true)}
            type="button"
          >
            Accept
          </Button>
          <Button
            className="h-7 px-2 py-0 text-xs border-[var(--error)]/30 bg-[var(--error)]/10 text-[var(--error)] hover:bg-[var(--error)]/20"
            disabled={isResponding}
            onClick={() => void respond(false)}
            type="button"
          >
            Reject
          </Button>
        </div>
      }
    />
  );
}
