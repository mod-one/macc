import React from 'react';
import { buildUrl } from '../api/client';
import { resolveApiBaseUrl } from '../api/config';
import type { ApiEventPayload, ApiEventStreamMessage, ApiEventStreamName } from '../api/models';

export type EventSourceConnectionState = 'connecting' | 'open' | 'closed';

export interface UseEventSourceOptions {
  maxEvents?: number;
  baseUrl?: string;
}

export interface UseEventSourceResult {
  connectionState: EventSourceConnectionState;
  events: ApiEventStreamMessage[];
  replayGapDetected: boolean;
  reconnectAttempt: number;
}

const DEFAULT_MAX_EVENTS = 250;
const STREAM_TYPES: ApiEventStreamName[] = ['coordinator_event', 'heartbeat'];
const INITIAL_RECONNECT_DELAY_MS = 1_000;
const MAX_RECONNECT_DELAY_MS = 30_000;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isApiEventPayload(value: unknown): value is ApiEventPayload {
  return (
    isRecord(value) &&
    typeof value.schema_version === 'string' &&
    typeof value.event_id === 'string' &&
    typeof value.seq === 'number' &&
    typeof value.ts === 'string' &&
    typeof value.source === 'string' &&
    typeof value.type === 'string' &&
    typeof value.status === 'string'
  );
}

function parseStreamPayload(data: string): ApiEventPayload | null {
  try {
    const parsed = JSON.parse(data) as unknown;
    return isApiEventPayload(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

function normalizeConnectionState(source: EventSource): EventSourceConnectionState {
  if (source.readyState === EventSource.OPEN) {
    return 'open';
  }
  if (source.readyState === EventSource.CLOSED) {
    return 'closed';
  }
  return 'connecting';
}

function buildEventSourceUrl(
  path: string,
  baseUrl: string | undefined,
  lastEventId: string | null,
): string {
  const resolvedBaseUrl = resolveApiBaseUrl(baseUrl);
  const url = new URL(buildUrl(path, resolvedBaseUrl), resolvedBaseUrl ?? window.location.origin);

  if (lastEventId) {
    url.searchParams.set('last_event_id', lastEventId);
  } else {
    url.searchParams.delete('last_event_id');
  }

  if (!resolvedBaseUrl) {
    return `${url.pathname}${url.search}`;
  }

  return url.toString();
}

function reconnectDelayMs(attempt: number): number {
  return Math.min(INITIAL_RECONNECT_DELAY_MS * 2 ** attempt, MAX_RECONNECT_DELAY_MS);
}

export function useEventSource(
  path: string,
  options: UseEventSourceOptions = {},
): UseEventSourceResult {
  const [connectionState, setConnectionState] = React.useState<EventSourceConnectionState>(
    'connecting',
  );
  const [events, setEvents] = React.useState<ApiEventStreamMessage[]>([]);
  const [replayGapDetected, setReplayGapDetected] = React.useState(false);
  const [reconnectAttempt, setReconnectAttempt] = React.useState(0);

  React.useEffect(() => {
    const maxEvents = options.maxEvents ?? DEFAULT_MAX_EVENTS;
    setConnectionState('connecting');
    setEvents([]);
    setReplayGapDetected(false);
    setReconnectAttempt(0);

    let active = true;
    let source: EventSource | null = null;
    let retryTimer: ReturnType<typeof window.setTimeout> | null = null;
    let lastEventId: string | null = null;
    let lastSeenSeq: number | null = null;
    let retryCount = 0;
    let awaitingReplay = false;

    const clearRetryTimer = (): void => {
      if (retryTimer !== null) {
        window.clearTimeout(retryTimer);
        retryTimer = null;
      }
    };

    const connect = (): void => {
      if (!active) {
        return;
      }

      const nextSource = new EventSource(buildEventSourceUrl(path, options.baseUrl, lastEventId));
      source = nextSource;
      setConnectionState('connecting');

      const handleOpen = (): void => {
        retryCount = 0;
        setReconnectAttempt(0);
        setConnectionState('open');
      };

      const scheduleReconnect = (): void => {
        if (!active) {
          return;
        }

        const delay = reconnectDelayMs(retryCount);
        setReconnectAttempt(retryCount + 1);
        retryTimer = window.setTimeout(() => {
          retryTimer = null;
          connect();
        }, delay);
        retryCount += 1;
      };

      const handleError = (): void => {
        if (source !== nextSource) {
          return;
        }

        const nextState = normalizeConnectionState(nextSource);
        setConnectionState(nextState);

        if (!active || retryTimer !== null) {
          return;
        }

        if (lastEventId) {
          awaitingReplay = true;
        }

        cleanupSource(nextSource);
        setConnectionState('connecting');
        scheduleReconnect();
      };

      const handleStreamEvent =
        (stream: ApiEventStreamName) =>
        (event: MessageEvent<string>): void => {
          const payload = parseStreamPayload(event.data);
          if (!payload) {
            return;
          }

          if (awaitingReplay && lastSeenSeq !== null && payload.seq > lastSeenSeq + 1) {
            setReplayGapDetected(true);
          }

          awaitingReplay = false;
          lastSeenSeq = payload.seq;
          lastEventId = event.lastEventId || payload.event_id || lastEventId;

          setEvents((currentEvents) => {
            const nextEvent: ApiEventStreamMessage = {
              stream,
              eventId: event.lastEventId || null,
              receivedAt: new Date().toISOString(),
              payload,
            };
            const nextEvents = [nextEvent, ...currentEvents];
            return nextEvents.slice(0, maxEvents);
          });
        };

      const cleanupSource = (target: EventSource): void => {
        if (source === target) {
          source = null;
        }
        target.removeEventListener('open', handleOpen);
        target.removeEventListener('error', handleError);
        for (const { stream, handler } of streamHandlers) {
          target.removeEventListener(stream, handler as EventListener);
        }
        target.close();
      };

      nextSource.addEventListener('open', handleOpen);
      nextSource.addEventListener('error', handleError);

      const streamHandlers = STREAM_TYPES.map((stream) => {
        const handler = handleStreamEvent(stream);
        nextSource.addEventListener(stream, handler as EventListener);
        return { stream, handler };
      });
    };

    connect();

    return () => {
      active = false;
      clearRetryTimer();
      source?.close();
    };
  }, [options.baseUrl, options.maxEvents, path]);

  return { connectionState, events, replayGapDetected, reconnectAttempt };
}
