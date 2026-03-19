import React from 'react';
import { buildUrl } from '../api/client';
import type { ApiEventPayload, ApiEventStreamMessage, ApiEventStreamName } from '../api/models';

export type EventSourceConnectionState = 'connecting' | 'open' | 'closed';

export interface UseEventSourceOptions {
  maxEvents?: number;
  baseUrl?: string;
}

export interface UseEventSourceResult {
  connectionState: EventSourceConnectionState;
  events: ApiEventStreamMessage[];
}

const DEFAULT_MAX_EVENTS = 250;
const STREAM_TYPES: ApiEventStreamName[] = ['coordinator_event', 'heartbeat'];

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

export function useEventSource(
  path: string,
  options: UseEventSourceOptions = {},
): UseEventSourceResult {
  const [connectionState, setConnectionState] = React.useState<EventSourceConnectionState>(
    'connecting',
  );
  const [events, setEvents] = React.useState<ApiEventStreamMessage[]>([]);

  React.useEffect(() => {
    const maxEvents = options.maxEvents ?? DEFAULT_MAX_EVENTS;
    const source = new EventSource(buildUrl(path, options.baseUrl));

    setConnectionState('connecting');
    setEvents([]);

    const handleOpen = (): void => {
      setConnectionState('open');
    };

    const handleError = (): void => {
      setConnectionState(normalizeConnectionState(source));
    };

    const handleStreamEvent =
      (stream: ApiEventStreamName) =>
      (event: MessageEvent<string>): void => {
        const payload = parseStreamPayload(event.data);
        if (!payload) {
          return;
        }

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

    source.addEventListener('open', handleOpen);
    source.addEventListener('error', handleError);

    const streamHandlers = STREAM_TYPES.map((stream) => {
      const handler = handleStreamEvent(stream);
      source.addEventListener(stream, handler as EventListener);
      return { stream, handler };
    });

    return () => {
      source.removeEventListener('open', handleOpen);
      source.removeEventListener('error', handleError);
      for (const { stream, handler } of streamHandlers) {
        source.removeEventListener(stream, handler as EventListener);
      }
      source.close();
    };
  }, [options.baseUrl, options.maxEvents, path]);

  return { connectionState, events };
}
