import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { getStatus, postCoordinatorAction } from './api/client';
import type { ApiCoordinatorStatus } from './api/models';
import { useCoordinatorStore } from './store';

vi.mock('./api/client', () => ({
  getStatus: vi.fn(),
  postCoordinatorAction: vi.fn(),
}));

const mockedGetStatus = vi.mocked(getStatus);
const mockedPostCoordinatorAction = vi.mocked(postCoordinatorAction);

const coordinatorStatus: ApiCoordinatorStatus = {
  total: 12,
  todo: 4,
  active: 2,
  blocked: 1,
  merged: 5,
  paused: false,
  pause_reason: null,
  pause_task_id: null,
  pause_phase: null,
  latest_error: null,
  failure_report: null,
  throttled_tools: [],
  effective_max_parallel: 3,
};

describe('useCoordinatorStore', () => {
  beforeEach(() => {
    mockedGetStatus.mockReset();
    mockedPostCoordinatorAction.mockReset();
    useCoordinatorStore.setState({
      status: null,
      loadError: null,
      isLoadingStatus: true,
      pendingAction: null,
    });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('loads status into the store', async () => {
    mockedGetStatus.mockResolvedValue(coordinatorStatus);

    const result = await useCoordinatorStore.getState().loadStatus();

    expect(result).toEqual(coordinatorStatus);
    expect(useCoordinatorStore.getState()).toMatchObject({
      status: coordinatorStatus,
      loadError: null,
      isLoadingStatus: false,
      pendingAction: null,
    });
  });

  it('refreshes status after an action when the API omits a snapshot', async () => {
    mockedPostCoordinatorAction.mockResolvedValue({ resumed: true });
    mockedGetStatus.mockResolvedValue(coordinatorStatus);

    const result = await useCoordinatorStore.getState().runAction('run');

    expect(result).toEqual({ resumed: true });
    expect(mockedGetStatus).toHaveBeenCalledTimes(1);
    expect(useCoordinatorStore.getState()).toMatchObject({
      status: coordinatorStatus,
      loadError: null,
      pendingAction: null,
    });
  });
});
