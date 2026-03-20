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

function HookHarness() {
  const { connectionState, events } = useEventSource('/events');

  return (
    <div>
      <span>{connectionState}</span>
      <span>{events.length}</span>
      <span>{events[0]?.payload.type ?? 'none'}</span>
    </div>
  );
}

describe('useEventSource', () => {
  const originalEventSource = globalThis.EventSource;

  beforeEach(() => {
    MockEventSource.instances = [];
    vi.stubGlobal('EventSource', MockEventSource);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    globalThis.EventSource = originalEventSource;
  });

  it('subscribes to the stream and stores incoming events', () => {
    render(<HookHarness />);

    expect(MockEventSource.instances).toHaveLength(1);
    expect(MockEventSource.instances[0]?.url).toBe('/api/v1/events');

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

    expect(screen.getByText('open')).toBeInTheDocument();
    expect(screen.getByText('1')).toBeInTheDocument();
    expect(screen.getByText('task_transition')).toBeInTheDocument();
  });
});
