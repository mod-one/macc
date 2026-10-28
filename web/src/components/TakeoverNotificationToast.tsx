import { useEffect, useRef, useState } from 'react';
import {
  getWebClientId,
  heartbeatProjectOwnership,
  respondProjectTakeover,
} from '../api/client';
import { useEventSource } from '../hooks/useEventSource';
import {
  useIsOwner,
  useOwnershipRecord,
  useOwnershipStore,
} from '../stores/ownershipStore';
import { Toast } from './Toast';
import { Button } from './Button';

interface PendingTakeoverRequest {
  requestId: string;
  requesterClientId: string;
  requestedAt: string;
}

function extractTakeoverRequestFromEvent(p: Record<string, unknown>): PendingTakeoverRequest | null {
  if (typeof p.request_id === 'string') {
    const requesterClientId =
      typeof p.requester_client_id === 'string'
        ? p.requester_client_id
        : typeof p.requester === 'object' &&
            p.requester !== null &&
            typeof (p.requester as Record<string, unknown>).client_id === 'string'
          ? ((p.requester as Record<string, unknown>).client_id as string)
          : 'unknown';
    return {
      requestId: p.request_id,
      requesterClientId,
      requestedAt: typeof p.ts === 'string' ? p.ts : '',
    };
  }
  const innerPayload = p.payload;
  if (typeof innerPayload === 'object' && innerPayload !== null) {
    const msg = (innerPayload as Record<string, unknown>).message;
    if (typeof msg === 'string') {
      try {
        const parsed = JSON.parse(msg) as Record<string, unknown>;
        if (typeof parsed.request_id === 'string') {
          const client = parsed.client as Record<string, unknown> | undefined;
          const requesterClientId =
            typeof client?.client_id === 'string' ? client.client_id : 'unknown';
          return {
            requestId: parsed.request_id,
            requesterClientId,
            requestedAt: typeof p.ts === 'string' ? p.ts : '',
          };
        }
      } catch {
        // Ignore malformed message JSON
      }
    }
  }
  return null;
}

export function TakeoverNotificationToast() {
  const isOwner = useIsOwner();
  const record = useOwnershipRecord();
  const { events } = useEventSource('/events', { maxEvents: 20 });
  const [pending, setPending] = useState<PendingTakeoverRequest | null>(null);
  const [open, setOpen] = useState(false);
  const [isResponding, setIsResponding] = useState(false);
  const processedEventIds = useRef(new Set<string>());
  const dismissedRequestId = useRef<string | null>(null);
  const heartbeatedRequestId = useRef<string | null>(null);

  useEffect(() => {
    if (!isOwner) {
      setPending(null);
      setOpen(false);
      return;
    }
    const activeReq = record?.takeover_request;
    if (activeReq) {
      if (heartbeatedRequestId.current !== activeReq.request_id) {
        heartbeatedRequestId.current = activeReq.request_id;
        void heartbeatProjectOwnership({ clientId: getWebClientId() }).catch(() => undefined);
      }
      if (dismissedRequestId.current !== activeReq.request_id) {
        setPending({
          requestId: activeReq.request_id,
          requesterClientId: activeReq.requester.client_id,
          requestedAt: activeReq.requested_at,
        });
        setOpen(true);
      }
      return;
    } else if (record) {
      setPending(null);
      setOpen(false);
    }
  }, [isOwner, record]);

  useEffect(() => {
    if (!isOwner) return;
    for (const entry of events) {
      const p = entry.payload;
      if (p.type !== 'takeover_requested') continue;
      const eventId = p.event_id;
      if (processedEventIds.current.has(eventId)) continue;
      processedEventIds.current.add(eventId);
      const extracted = extractTakeoverRequestFromEvent(p as unknown as Record<string, unknown>);
      if (extracted && dismissedRequestId.current !== extracted.requestId) {
        if (heartbeatedRequestId.current !== extracted.requestId) {
          heartbeatedRequestId.current = extracted.requestId;
          void heartbeatProjectOwnership({ clientId: getWebClientId() }).catch(() => undefined);
        }
        setPending(extracted);
        setOpen(true);
      }
    }
  }, [events, isOwner]);

  const respond = async (accept: boolean) => {
    if (!pending) return;
    setIsResponding(true);
    dismissedRequestId.current = pending.requestId;
    try {
      await respondProjectTakeover(pending.requestId, accept);
      await useOwnershipStore.getState().fetchRecords();
    } catch {
      // Silently ignore network errors; ownership state will sync on next event
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
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen);
        if (!nextOpen && pending) {
          dismissedRequestId.current = pending.requestId;
        }
      }}
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
