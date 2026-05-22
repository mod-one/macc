import { describe, expect, it, vi, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useNotificationCenter } from './useNotificationCenter';
import { useNotificationStore } from '../stores/notificationStore';
import { useEventSource } from './useEventSource';
import type { ApiEventPayload, ApiEventStreamMessage } from '../api/models';

vi.mock('./useEventSource', () => ({
  useEventSource: vi.fn(() => ({ events: [] })),
}));

const mockUseEventSource = vi.mocked(useEventSource);

function event(payload: Partial<ApiEventPayload> & Pick<ApiEventPayload, 'seq' | 'type' | 'status'>): ApiEventStreamMessage {
  return {
    stream: 'coordinator_event',
    eventId: `event-${payload.seq}`,
    receivedAt: '2026-05-22T00:00:00Z',
    payload: {
      schema_version: '1',
      event_id: `event-${payload.seq}`,
      ts: '2026-05-22T00:00:00Z',
      source: 'coordinator',
      ...payload,
    },
  };
}

function mockEvents(events: ApiEventStreamMessage[]): void {
  mockUseEventSource.mockReturnValue({
    connectionState: 'open',
    events,
    replayGapDetected: false,
    reconnectAttempt: 0,
  });
}

describe('useNotificationCenter', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useNotificationStore.setState({
      notifications: [],
      unreadCount: 0,
      isOpen: false,
    });
    mockEvents([]);
  });

  it('maps "failed" event to error notification', () => {
    mockEvents([
      event({
        seq: 1,
        type: 'failed',
        msg: 'Task failed',
        status: 'error',
      }),
    ]);

    renderHook(() => useNotificationCenter());

    const state = useNotificationStore.getState();
    expect(state.notifications).toHaveLength(1);
    expect(state.notifications[0]).toMatchObject({
      type: 'error',
      title: 'Action Failed',
      message: 'Task failed',
    });
  });

  it('maps "commit_created" event to success notification', () => {
    mockEvents([
      event({
        seq: 2,
        type: 'commit_created',
        task_id: 'TASK-1',
        status: 'success',
      }),
    ]);

    renderHook(() => useNotificationCenter());

    const state = useNotificationStore.getState();
    expect(state.notifications).toHaveLength(1);
    expect(state.notifications[0]).toMatchObject({
      type: 'success',
      title: 'Task Merged',
      message: 'Task TASK-1 was successfully merged.',
    });
  });

  it('maps coordinator "paused" status to warning notification', () => {
    mockEvents([
      event({
        seq: 3,
        status: 'paused',
        msg: 'Manual pause',
        type: 'coordinator_status',
      }),
    ]);

    renderHook(() => useNotificationCenter());

    const state = useNotificationStore.getState();
    expect(state.notifications).toHaveLength(1);
    expect(state.notifications[0]).toMatchObject({
      type: 'warning',
      title: 'Coordinator Paused',
      message: 'Manual pause',
    });
  });

  it('only processes new events based on seq', () => {
    const { rerender } = renderHook(() => useNotificationCenter());

    // First event
    mockEvents([
      event({
        seq: 1,
        type: 'failed',
        msg: 'Error 1',
        status: 'error',
      }),
    ]);
    rerender();

    expect(useNotificationStore.getState().notifications).toHaveLength(1);

    // Second event (including the first one)
    mockEvents([
      event({
        seq: 2,
        type: 'failed',
        msg: 'Error 2',
        status: 'error',
      }),
      event({
        seq: 1,
        type: 'failed',
        msg: 'Error 1',
        status: 'error',
      }),
    ]);
    rerender();

    expect(useNotificationStore.getState().notifications).toHaveLength(2);
    expect(useNotificationStore.getState().notifications[0].message).toBe('Error 2');
  });
});
