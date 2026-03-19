import { create } from 'zustand';
import { getStatus, postCoordinatorAction } from './api/client';
import type {
  ApiCoordinatorAction,
  ApiCoordinatorCommandResult,
  ApiCoordinatorStatus,
} from './api/models';

interface CoordinatorStoreState {
  status: ApiCoordinatorStatus | null;
  loadError: string | null;
  isLoadingStatus: boolean;
  pendingAction: ApiCoordinatorAction | null;
}

interface CoordinatorStoreActions {
  loadStatus: (signal?: AbortSignal) => Promise<ApiCoordinatorStatus | null>;
  runAction: (action: ApiCoordinatorAction) => Promise<ApiCoordinatorCommandResult>;
}

type CoordinatorStore = CoordinatorStoreState & CoordinatorStoreActions;

const initialState: CoordinatorStoreState = {
  status: null,
  loadError: null,
  isLoadingStatus: true,
  pendingAction: null,
};

export const useCoordinatorStore = create<CoordinatorStore>((set) => ({
  ...initialState,
  async loadStatus(signal) {
    set({ isLoadingStatus: true });

    try {
      const status = await getStatus({ signal });
      set({
        status,
        loadError: null,
      });
      return status;
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') {
        return null;
      }

      const message = error instanceof Error ? error.message : 'Unexpected coordinator error.';
      set({ loadError: message });
      throw error;
    } finally {
      set({ isLoadingStatus: false });
    }
  },
  async runAction(action) {
    set({ pendingAction: action });

    try {
      const result = await postCoordinatorAction(action);

      if (result.status) {
        set({
          status: result.status,
          loadError: null,
        });
      } else {
        const status = await getStatus();
        set({
          status,
          loadError: null,
        });
      }

      return result;
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unexpected coordinator error.';
      set({ loadError: message });
      throw error;
    } finally {
      set({ pendingAction: null });
    }
  },
}));

