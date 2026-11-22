import { useCallback, useEffect, useRef } from 'react';
import { create } from 'zustand';
import { getDecisions, type Decision, type DecisionSnapshot } from '../api/decisions';
import type { ApiCoordinatorStatus } from '../api/models';

export function awaitingApprovals(status: ApiCoordinatorStatus | null) {
  return (status?.pending_approvals ?? []).filter(gate => gate.awaiting_decision);
}

export function awaitingDecisions(snapshot: DecisionSnapshot | null) {
  return (snapshot?.decisions ?? []).filter(d => d.status === 'pending_human' || d.status === 'blocked_external');
}

// Shared with the dialogs already mounted in Layout. Opening the Inbox or
// receiving a fresh snapshot does not select an approval on the user's behalf.
interface InboxState {
  decisions: DecisionSnapshot | null;
  decisionError: string;
  decisionToInspect: Decision | null;
  approval: { id: string; manual: boolean } | null;
}

export const useInboxStore = create<InboxState>(() => ({
  decisions: null,
  decisionError: '',
  decisionToInspect: null,
  approval: null,
}));

export function openApproval(id: string) {
  useInboxStore.setState({ approval: { id, manual: true } });
}

// The global decision dialog owns polling. The Inbox reads the same snapshot.
export function useInboxDecisions() {
  const snapshot = useInboxStore(s => s.decisions);
  const loadError = useInboxStore(s => s.decisionError);
  const pending = useRef<AbortController | null>(null);
  const refresh = useCallback(async (background = false) => {
    if (background && pending.current) return null;
    pending.current?.abort();
    const request = new AbortController();
    pending.current = request;
    try {
      const data = await getDecisions(request.signal);
      if (request.signal.aborted) return null;
      useInboxStore.setState({ decisions: data, decisionError: '' });
      return data;
    } catch (error) {
      if (!request.signal.aborted) useInboxStore.setState({ decisionError: String(error) });
      return null;
    } finally {
      if (pending.current === request) pending.current = null;
    }
  }, []);
  useEffect(() => {
    let active = true;
    let timer: number;
    const poll = async () => {
      await refresh(true);
      if (active) timer = window.setTimeout(() => void poll(), 3000);
    };
    void poll();
    return () => {
      pending.current?.abort();
      pending.current = null;
      active = false;
      window.clearTimeout(timer);
      useInboxStore.setState({ decisions: null, decisionError: '', decisionToInspect: null });
    };
  }, [refresh]);
  return { snapshot, loadError, refresh };
}
