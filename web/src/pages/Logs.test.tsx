import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Logs from './Logs';

type EventHandler = (event: MessageEvent<string> | Event) => void;

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

  emitError(nextState: number): void {
    this.readyState = nextState;
    this.emit('error', new Event('error'));
  }

  emitMessage(type: string, payload: Record<string, unknown>, lastEventId = ''): void {
    const event = {
      data: JSON.stringify(payload),
      lastEventId,
    } as MessageEvent<string>;
    this.emit(type, event);
  }

  private emit(type: string, event: MessageEvent<string> | Event): void {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

describe('Logs page', () => {
  const originalEventSource = globalThis.EventSource;

  beforeEach(() => {
    MockEventSource.instances = [];
    vi.stubGlobal('EventSource', MockEventSource);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    globalThis.EventSource = originalEventSource;
  });

  it('renders streamed events and filters by event type', async () => {
    render(<Logs />);

    expect(screen.getByText(/waiting for the first event/i)).toBeInTheDocument();
    expect(MockEventSource.instances).toHaveLength(1);
    expect(MockEventSource.instances[0]?.url).toBe('/api/v1/events');

    const source = MockEventSource.instances[0]!;
    source.emitOpen();

    expect(await screen.findByText('open')).toBeInTheDocument();

    source.emitMessage(
      'coordinator_event',
      {
        schema_version: '1',
        event_id: 'evt-1',
        seq: 4,
        ts: '2026-03-19T10:00:00Z',
        source: 'coordinator',
        type: 'task_transition',
        status: 'ok',
        task_id: 'WEB-FRONTEND-005',
        phase: 'dev',
        detail: 'Task moved to dev.',
      },
      'evt-1',
    );
    source.emitMessage(
      'heartbeat',
      {
        schema_version: '1',
        event_id: 'hb-1',
        seq: 5,
        ts: '2026-03-19T10:00:05Z',
        source: 'coordinator',
        type: 'heartbeat',
        status: 'ok',
      },
      'hb-1',
    );

    expect(await screen.findByText('task_transition')).toBeInTheDocument();
    expect(screen.getByText('heartbeat')).toBeInTheDocument();
    expect(screen.getByText(/task moved to dev/i)).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText(/event type/i), {
      target: { value: 'heartbeat' },
    });

    expect(screen.queryByText('task_transition')).not.toBeInTheDocument();
    expect(screen.getByText('heartbeat')).toBeInTheDocument();
  });

  it('shows reconnecting and closed states from EventSource errors', async () => {
    render(<Logs />);

    const source = MockEventSource.instances[0]!;
    source.emitError(MockEventSource.CONNECTING);
    expect(await screen.findByText('connecting')).toBeInTheDocument();

    source.emitError(MockEventSource.CLOSED);
    expect(await screen.findByText('closed')).toBeInTheDocument();
  });
});
