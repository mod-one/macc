import { useEffect } from 'react';
import { create } from 'zustand';
import {
  claimProjectOwnership,
  getWebClientId,
  heartbeatProjectOwnership,
  listProcessOwnership,
  setWebOwnershipMode,
} from '../api/client';
import type { ApiOwnershipRecord, ApiOwnershipStatus, ApiProcessKind } from '../api/models';

interface OwnershipStoreState {
  records: ApiOwnershipRecord[];
  isLoading: boolean;
  isReady: boolean;
  error: string | null;
  lastHeartbeatedRequestId: string | null;
}

interface OwnershipStoreActions {
  fetchRecords: () => Promise<void>;
}

export const useOwnershipStore = create<OwnershipStoreState & OwnershipStoreActions>((set, get) => ({
  records: [],
  isLoading: false,
  isReady: false,
  error: null,
  lastHeartbeatedRequestId: null,

  fetchRecords: async () => {
    set({ isLoading: true });
    try {
      let records = await listProcessOwnership();
      const clientId = getWebClientId();
      let projectRecord =
        records.find((r) => r.process.kind === 'Project') ??
        records.find((r) => r.process.kind === 'Coordinator') ??
        records[0] ??
        null;

      // Single active Web client automatically becomes owner when the project is unowned.
      if (projectRecord && !projectRecord.owner) {
        try {
          await claimProjectOwnership({ clientId });
          records = await listProcessOwnership();
          projectRecord =
            records.find((r) => r.process.kind === 'Project') ??
            records.find((r) => r.process.kind === 'Coordinator') ??
            records[0] ??
            null;
        } catch {
          // Ignore claim error; fall back to current record snapshot
        }
      }

      let nextHeartbeatedRequestId = get().lastHeartbeatedRequestId;
      if (projectRecord) {
        if (projectRecord.owner?.client_id === clientId) {
          setWebOwnershipMode('owner');
          const activeReq = projectRecord.takeover_request;
          if (activeReq) {
            if (nextHeartbeatedRequestId !== activeReq.request_id) {
              nextHeartbeatedRequestId = activeReq.request_id;
              void heartbeatProjectOwnership({ clientId }).catch(() => undefined);
            }
          } else {
            nextHeartbeatedRequestId = null;
          }
        } else {
          setWebOwnershipMode('viewer');
          nextHeartbeatedRequestId = null;
        }
      } else {
        setWebOwnershipMode('unknown');
        nextHeartbeatedRequestId = null;
      }
      set({
        records,
        error: null,
        isReady: true,
        lastHeartbeatedRequestId: nextHeartbeatedRequestId,
      });
    } catch (err) {
      set({ error: err instanceof Error ? err.message : 'Failed to load ownership records' });
    } finally {
      set({ isLoading: false });
    }
  },
}));

/**
 * Performs an initial ownership state fetch on mount if not yet loaded.
 * Subsequent updates are pushed reactively via SSE ownership events.
 */
export function useOwnershipPolling(): void {
  const isReady = useOwnershipStore((state) => state.isReady);
  const fetchRecords = useOwnershipStore((state) => state.fetchRecords);
  useEffect(() => {
    if (!isReady) {
      void fetchRecords();
    }
  }, [isReady, fetchRecords]);
}

function findRecordByKind(
  records: ApiOwnershipRecord[],
  kind?: ApiProcessKind,
): ApiOwnershipRecord | null {
  if (kind) {
    return records.find((r) => r.process.kind === kind) ?? null;
  }
  return (
    records.find((r) => r.process.kind === 'Project') ??
    records.find((r) => r.process.kind === 'Coordinator') ??
    records[0] ??
    null
  );
}

export function useIsOwner(kind?: ApiProcessKind): boolean {
  const records = useOwnershipStore((state) => state.records);
  const isReady = useOwnershipStore((state) => state.isReady);
  const clientId = getWebClientId();
  if (!isReady) return true;
  const record = findRecordByKind(records, kind);
  if (!record) return false;
  return record.owner?.client_id === clientId;
}

export function useOwnershipRecord(kind?: ApiProcessKind): ApiOwnershipRecord | null {
  const records = useOwnershipStore((state) => state.records);
  return findRecordByKind(records, kind);
}

export function useOwnershipStatus(kind?: ApiProcessKind): ApiOwnershipStatus {
  const records = useOwnershipStore((state) => state.records);
  const isReady = useOwnershipStore((state) => state.isReady);
  const clientId = getWebClientId();
  if (!isReady) return 'unregistered';
  const record = findRecordByKind(records, kind);
  if (!record) return 'unregistered';
  if (record.owner?.client_id === clientId) return 'owner';
  const isViewer = record.viewers.some((v) => v.client_id === clientId);
  if (isViewer) return 'viewer';
  return 'unregistered';
}
