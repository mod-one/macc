import { act, render, screen } from '@testing-library/react';
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { useEventSource } from './useEventSource';

type EventHandler = (event: Event | MessageEvent<string>) => void;

class MockEventSource {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSED = 2;

  static instances: MockEventSource[] = [];

  readonly url: string;
  readyState = MockEventSource.CONNECTING;
  private readonly listeners = new Map<string, Set<EventHandler>>();

  constructor(url: string) {
    this.url = url;
    MockEventSource.instances.push(this);
  }

  addEventListener(type: string, listener: EventHandler): void {
    const listeners = this.listeners.get(type) ?? new Set<EventHandler>();
    listeners.add(listener);
    this.listeners.set(type, listeners);
  }

  removeEventListener(type: string, listener: EventHandler): void {
    this.listeners.get(type)?.delete(listener);
  }

  close(): void {
    this.readyState = MockEventSource.CLOSED;
  }

  emitOpen(): void {
    this.readyState = MockEventSource.OPEN;
    this.emit('open', new Event('open'));
  }

  emitError(nextState = MockEventSource.CONNECTING): void {
    this.readyState = nextState;
    this.emit('error', new Event('error'));
  }

  emitMessage(type: string, payload: Record<string, unknown>, lastEventId = ''): void {
    this.emit(
      type,
      {
        data: JSON.stringify(payload),
        lastEventId,
      } as MessageEvent<string>,
    );
  }

  private emit(type: string, event: Event | MessageEvent<string>): void {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

interface HookHarnessProps {
  maxEvents?: number;
}

function HookHarness({ maxEvents }: HookHarnessProps = {}) {
  const { connectionState, events, replayGapDetected, reconnectAttempt } = useEventSource('/events', {
    maxEvents,
  });
  const eventIds = events.map((event) => event.payload.event_id).join(',');

  return (
    <div>
      <span data-testid="connection-state">{connectionState}</span>
      <span data-testid="event-count">{events.length}</span>
      <span data-testid="event-type">{events[0]?.payload.type ?? 'none'}</span>
      <span data-testid="replay-gap">{String(replayGapDetected)}</span>
      <span data-testid="reconnect-attempt">{reconnectAttempt}</span>
      <span data-testid="event-ids">{eventIds || 'none'}</span>
    </div>
  );
}

describe('useEventSource', () => {
  const originalEventSource = globalThis.EventSource;

  beforeEach(() => {
    MockEventSource.instances = [];
    vi.stubGlobal('EventSource', MockEventSource);
    window.sessionStorage.clear();
    window.sessionStorage.setItem('macc_client_id', 'event-source-client');
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    globalThis.EventSource = originalEventSource;
  });

  it('subscribes to the stream and stores incoming events', () => {
    render(<HookHarness />);

    expect(MockEventSource.instances).toHaveLength(1);
    expect(MockEventSource.instances[0]?.url).toBe('/api/v1/events?client_id=event-source-client');

    act(() => {
      MockEventSource.instances[0]?.emitOpen();
      MockEventSource.instances[0]?.emitMessage(
        'coordinator_event',
        {
          schema_version: '1',
          event_id: 'evt-1',
          seq: 1,
          ts: '2026-03-20T00:00:00Z',
          source: 'coordinator',
          type: 'task_transition',
          status: 'ok',
        },
        'evt-1',
      );
    });

    expect(screen.getByTestId('connection-state')).toHaveTextContent('open');
    expect(screen.getByTestId('event-count')).toHaveTextContent('1');
    expect(screen.getByTestId('event-type')).toHaveTextContent('task_transition');
  });

  it('reconnects after disconnect and resumes with last_event_id', async () => {
    vi.useFakeTimers();
    render(<HookHarness />);

    const source = MockEventSource.instances[0]!;
    act(() => {
      source.emitMessage(
        'coordinator_event',
        {
          schema_version: '1',
          event_id: 'evt-8',
          seq: 8,
          ts: '2026-03-20T00:00:00Z',
          source: 'coordinator',
          type: 'task_transition',
          status: 'ok',
        },
        'evt-8',
      );
      source.emitError();
    });

    expect(screen.getByTestId('connection-state')).toHaveTextContent('connecting');
    expect(screen.getByTestId('reconnect-attempt')).toHaveTextContent('1');

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000);
    });

    expect(MockEventSource.instances).toHaveLength(2);
    expect(MockEventSource.instances[1]?.url).toBe(
      '/api/v1/events?last_event_id=evt-8&client_id=event-source-client',
    );
    vi.useRealTimers();
  });

  it('uses exponential backoff for repeated reconnect attempts', async () => {
    vi.useFakeTimers();
    render(<HookHarness />);

    act(() => {
      MockEventSource.instances[0]?.emitError();
    });
    expect(screen.getByTestId('reconnect-attempt')).toHaveTextContent('1');

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000);
    });
    expect(MockEventSource.instances).toHaveLength(2);

    act(() => {
      MockEventSource.instances[1]?.emitError();
    });
    expect(screen.getByTestId('reconnect-attempt')).toHaveTextContent('2');

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_999);
    });
    expect(MockEventSource.instances).toHaveLength(2);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(MockEventSource.instances).toHaveLength(3);
    vi.useRealTimers();
  });

  it('flags replay gaps when resumed stream skips sequence numbers', async () => {
    vi.useFakeTimers();
    render(<HookHarness />);

    const source = MockEventSource.instances[0]!;
    act(() => {
      source.emitMessage(
        'coordinator_event',
        {
          schema_version: '1',
          event_id: 'evt-8',
          seq: 8,
          ts: '2026-03-20T00:00:00Z',
          source: 'coordinator',
          type: 'task_transition',
          status: 'ok',
        },
        'evt-8',
      );
      source.emitError();
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000);
    });

    act(() => {
      const reconnected = MockEventSource.instances[1]!;
      reconnected.emitOpen();
      reconnected.emitMessage(
        'coordinator_event',
        {
          schema_version: '1',
          event_id: 'evt-12',
          seq: 12,
          ts: '2026-03-20T00:00:10Z',
          source: 'coordinator',
          type: 'task_transition',
          status: 'ok',
        },
        'evt-12',
      );
    });

    expect(screen.getByTestId('replay-gap')).toHaveTextContent('true');
    vi.useRealTimers();
  });

  it('truncates older events when buffer reaches maxEvents', () => {
    render(<HookHarness maxEvents={2} />);

    const source = MockEventSource.instances[0]!;
    act(() => {
      source.emitMessage(
        'coordinator_event',
        {
          schema_version: '1',
          event_id: 'evt-1',
          seq: 1,
          ts: '2026-03-20T00:00:00Z',
          source: 'coordinator',
          type: 'task_transition',
          status: 'ok',
        },
        'evt-1',
      );
      source.emitMessage(
        'coordinator_event',
        {
          schema_version: '1',
          event_id: 'evt-2',
          seq: 2,
          ts: '2026-03-20T00:00:01Z',
          source: 'coordinator',
          type: 'task_transition',
          status: 'ok',
        },
        'evt-2',
      );
      source.emitMessage(
        'coordinator_event',
        {
          schema_version: '1',
          event_id: 'evt-3',
          seq: 3,
          ts: '2026-03-20T00:00:02Z',
          source: 'coordinator',
          type: 'task_transition',
          status: 'ok',
        },
        'evt-3',
      );
    });

    expect(screen.getByTestId('event-count')).toHaveTextContent('2');
    expect(screen.getByTestId('event-ids')).toHaveTextContent('evt-3,evt-2');
  });
});
