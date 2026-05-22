import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { listProcessOwnership } from '../api/client';
import type { ApiOwnershipRecord } from '../api/models';
import { useOwnershipStore } from './ownershipStore';

vi.mock('../api/client', () => ({
  listProcessOwnership: vi.fn(),
  getWebClientId: vi.fn(() => 'test-client-id'),
  setWebOwnershipMode: vi.fn(),
}));

const mockedListProcessOwnership = vi.mocked(listProcessOwnership);

const makeRecord = (ownerClientId: string | null, viewerIds: string[] = []): ApiOwnershipRecord => ({
  process: { kind: 'Project', project_root: '/test', pid: null },
  owner: ownerClientId
    ? { client_id: ownerClientId, kind: 'Web', connected_at: '', last_heartbeat: '' }
    : null,
  viewers: viewerIds.map((id) => ({ client_id: id, kind: 'Web', connected_at: '', last_heartbeat: '' })),
  takeover_request: null,
  started_at: '',
});

describe('useOwnershipStore', () => {
  beforeEach(() => {
    mockedListProcessOwnership.mockReset();
    useOwnershipStore.setState({
      records: [],
      isLoading: false,
      isReady: false,
      error: null,
    });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('fetches and stores ownership records', async () => {
    const record = makeRecord('owner-client');
    mockedListProcessOwnership.mockResolvedValue([record]);

    await useOwnershipStore.getState().fetchRecords();

    const state = useOwnershipStore.getState();
    expect(state.records).toHaveLength(1);
    expect(state.records[0]).toEqual(record);
    expect(state.isReady).toBe(true);
    expect(state.isLoading).toBe(false);
    expect(state.error).toBeNull();
  });

  it('stores error message on fetch failure', async () => {
    mockedListProcessOwnership.mockRejectedValue(new Error('Network error'));

    await useOwnershipStore.getState().fetchRecords();

    const state = useOwnershipStore.getState();
    expect(state.error).toBe('Network error');
    expect(state.isReady).toBe(false);
    expect(state.isLoading).toBe(false);
  });

  it('stores generic error when non-Error thrown', async () => {
    mockedListProcessOwnership.mockRejectedValue('oops');

    await useOwnershipStore.getState().fetchRecords();

    expect(useOwnershipStore.getState().error).toBe('Failed to load ownership records');
  });
});
