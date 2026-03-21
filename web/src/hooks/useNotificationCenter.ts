import { useEffect, useRef, useCallback } from 'react';
import { useEventSource } from './useEventSource';
import { useNotificationStore } from '../stores/notificationStore';
import type { ApiEventPayload } from '../api/models';

export function useNotificationCenter() {
  const { events } = useEventSource('/events');
  const addNotification = useNotificationStore((state) => state.addNotification);
  const lastProcessedSeq = useRef<number>(-1);

  const processEvent = useCallback((payload: ApiEventPayload) => {
    const { type, status, msg, detail, task_id } = payload;

    // Task Failed / Blocked
    if (type === 'failed' || type === 'task_blocked' || type === 'command_error' || type === 'dispatch_failed') {
      addNotification({
        type: 'error',
        title: type === 'task_blocked' ? 'Task Blocked' : 'Action Failed',
        message: (msg as string) || (detail as string) || `Task ${task_id || 'unknown'} encountered an error.`,
      });
    } 
    // Task Merged / Completed
    else if (type === 'commit_created' || type === 'integrate_done') {
      addNotification({
        type: 'success',
        title: 'Task Merged',
        message: (msg as string) || (detail as string) || `Task ${task_id} was successfully merged.`,
      });
    }
    // Apply Completed
    else if (type === 'dispatch_complete') {
      addNotification({
        type: 'info',
        title: 'Apply Completed',
        message: (msg as string) || (detail as string) || 'Task dispatching complete.',
      });
    }
    // Coordinator Paused
    else if (status === 'paused' || type === 'quota_exhausted') {
      addNotification({
        type: 'warning',
        title: type === 'quota_exhausted' ? 'Quota Exhausted' : 'Coordinator Paused',
        message: (msg as string) || (detail as string) || 'The coordinator state has changed to paused.',
      });
    }
    // Doctor Issues (heuristically detect from event stream if possible)
    else if (payload.severity === 'error' || payload.severity === 'critical') {
      addNotification({
        type: 'error',
        title: 'Critical Issue',
        message: (msg as string) || (detail as string) || 'A critical system issue was detected.',
      });
    }
  }, [addNotification]);

  useEffect(() => {
    // Process new events from the stream
    // events array from useEventSource is [newest, ..., oldest]
    const sortedEvents = [...events].sort((a, b) => a.payload.seq - b.payload.seq);

    for (const event of sortedEvents) {
      if (event.payload.seq > lastProcessedSeq.current) {
        processEvent(event.payload);
        lastProcessedSeq.current = event.payload.seq;
      }
    }
  }, [events, processEvent]);
}
