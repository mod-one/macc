import { useEffect } from 'react';
import { create } from 'zustand';
import { listProcessOwnership, getWebClientId, setWebOwnershipMode } from '../api/client';
import type { ApiOwnershipRecord, ApiOwnershipStatus, ApiProcessKind } from '../api/models';

const POLL_INTERVAL_MS = 10_000;

interface OwnershipStoreState {
  records: ApiOwnershipRecord[];
  isLoading: boolean;
  isReady: boolean;
  error: string | null;
}

interface OwnershipStoreActions {
  fetchRecords: () => Promise<void>;
}

export const useOwnershipStore = create<OwnershipStoreState & OwnershipStoreActions>((set) => ({
  records: [],
  isLoading: false,
  isReady: false,
  error: null,

  fetchRecords: async () => {
    set({ isLoading: true });
    try {
      const records = await listProcessOwnership();
      const clientId = getWebClientId();
      const projectRecord =
        records.find((r) => r.process.kind === 'Project') ??
        records.find((r) => r.process.kind === 'Coordinator') ??
        records[0] ??
        null;
      if (projectRecord) {
        if (projectRecord.owner?.client_id === clientId) {
          setWebOwnershipMode('owner');
        } else {
          setWebOwnershipMode('viewer');
        }
      } else {
        setWebOwnershipMode('unknown');
      }
      set({ records, error: null, isReady: true });
    } catch (err) {
      set({ error: err instanceof Error ? err.message : 'Failed to load ownership records' });
    } finally {
      set({ isLoading: false });
    }
  },
}));

export function useOwnershipPolling(): void {
  const fetchRecords = useOwnershipStore((state) => state.fetchRecords);
  useEffect(() => {
    void fetchRecords();
    const timerId = window.setInterval(() => void fetchRecords(), POLL_INTERVAL_MS);
    return () => window.clearInterval(timerId);
  }, [fetchRecords]);
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
