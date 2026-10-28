import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  claimProjectOwnership,
  heartbeatProjectOwnership,
  listProcessOwnership,
} from '../api/client';
import type { ApiOwnershipRecord } from '../api/models';
import { useOwnershipStore } from './ownershipStore';

vi.mock('../api/client', () => ({
  claimProjectOwnership: vi.fn(),
  heartbeatProjectOwnership: vi.fn(() => Promise.resolve(undefined)),
  listProcessOwnership: vi.fn(),
  getWebClientId: vi.fn(() => 'test-client-id'),
  setWebOwnershipMode: vi.fn(),
}));

const mockedListProcessOwnership = vi.mocked(listProcessOwnership);
const mockedClaimProjectOwnership = vi.mocked(claimProjectOwnership);
const mockedHeartbeatProjectOwnership = vi.mocked(heartbeatProjectOwnership);

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
    mockedClaimProjectOwnership.mockReset();
    mockedHeartbeatProjectOwnership.mockReset();
    mockedHeartbeatProjectOwnership.mockResolvedValue(undefined);
    useOwnershipStore.setState({
      records: [],
      isLoading: false,
      isReady: false,
      error: null,
      lastHeartbeatedRequestId: null,
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

  it('automatically claims ownership when the project is unowned', async () => {
    const unowned = makeRecord(null);
    const claimed = makeRecord('test-client-id');
    mockedListProcessOwnership.mockResolvedValueOnce([unowned]).mockResolvedValueOnce([claimed]);
    mockedClaimProjectOwnership.mockResolvedValue({ status: 'owner' });

    await useOwnershipStore.getState().fetchRecords();

    expect(mockedClaimProjectOwnership).toHaveBeenCalledWith({ clientId: 'test-client-id' });
    expect(useOwnershipStore.getState().records[0]?.owner?.client_id).toBe('test-client-id');
  });

  it('automatically sends a heartbeat when a takeover request arrives for the active owner', async () => {
    const recordWithTakeover: ApiOwnershipRecord = {
      ...makeRecord('test-client-id'),
      takeover_request: {
        request_id: 'req-42',
        requester: {
          client_id: 'viewer-client',
          kind: 'Web',
          connected_at: '2026-05-21T00:00:01Z',
          last_heartbeat: '2026-05-21T00:00:01Z',
        },
        requested_at: '2026-05-21T00:00:02Z',
      },
    };
    mockedListProcessOwnership.mockResolvedValue([recordWithTakeover]);

    await useOwnershipStore.getState().fetchRecords();

    expect(mockedHeartbeatProjectOwnership).toHaveBeenCalledTimes(1);
    expect(mockedHeartbeatProjectOwnership).toHaveBeenCalledWith({ clientId: 'test-client-id' });
    expect(useOwnershipStore.getState().lastHeartbeatedRequestId).toBe('req-42');
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
