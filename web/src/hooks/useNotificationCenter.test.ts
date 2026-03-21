import { describe, expect, it, vi, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useNotificationCenter } from './useNotificationCenter';
import { useNotificationStore } from '../stores/notificationStore';
import { useEventSource } from './useEventSource';

vi.mock('./useEventSource', () => ({
  useEventSource: vi.fn(() => ({ events: [] })),
}));

describe('useNotificationCenter', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useNotificationStore.setState({
      notifications: [],
      unreadCount: 0,
      isOpen: false,
    });
    (useEventSource as any).mockReturnValue({ events: [] });
  });

  it('maps "failed" event to error notification', () => {
    (useEventSource as any).mockReturnValue({
      events: [
        {
          payload: {
            seq: 1,
            type: 'failed',
            msg: 'Task failed',
            status: 'error',
          },
        },
      ],
    });

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
    (useEventSource as any).mockReturnValue({
      events: [
        {
          payload: {
            seq: 2,
            type: 'commit_created',
            task_id: 'TASK-1',
            status: 'success',
          },
        },
      ],
    });

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
    (useEventSource as any).mockReturnValue({
      events: [
        {
          payload: {
            seq: 3,
            status: 'paused',
            msg: 'Manual pause',
            type: 'coordinator_status',
          },
        },
      ],
    });

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
    (useEventSource as any).mockReturnValue({
      events: [
        {
          payload: {
            seq: 1,
            type: 'failed',
            msg: 'Error 1',
            status: 'error',
          },
        },
      ],
    });
    rerender();

    expect(useNotificationStore.getState().notifications).toHaveLength(1);

    // Second event (including the first one)
    (useEventSource as any).mockReturnValue({
      events: [
        {
          payload: {
            seq: 2,
            type: 'failed',
            msg: 'Error 2',
            status: 'error',
          },
        },
        {
          payload: {
            seq: 1,
            type: 'failed',
            msg: 'Error 1',
            status: 'error',
          },
        },
      ],
    });
    rerender();

    expect(useNotificationStore.getState().notifications).toHaveLength(2);
    expect(useNotificationStore.getState().notifications[0].message).toBe('Error 2');
  });
});
