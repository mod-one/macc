import { create } from 'zustand';
import { getConfig, getLogs, getRegistryTasks, getWorktrees } from '../api/client';
import type { ApiConfigResponse, ApiLogFile, ApiRegistryTask, ApiWorktree } from '../api/models';

interface GlobalSearchState {
  tasks: ApiRegistryTask[];
  worktrees: ApiWorktree[];
  config: ApiConfigResponse | null;
  logs: ApiLogFile[];
  isLoading: boolean;
  error: string | null;
  lastLoadedAt: number | null;
  loadGlobalSearchData: (signal?: AbortSignal) => Promise<void>;
}

let pendingLoad: Promise<void> | null = null;

function formatLoadError(sources: Array<'tasks' | 'worktrees' | 'settings' | 'logs'>): string {
  return sources.length === 0
    ? 'Unable to load search data.'
    : `Unable to load ${sources.join(', ')} for search.`;
}

export const useGlobalSearchStore = create<GlobalSearchState>((set) => ({
  tasks: [],
  worktrees: [],
  config: null,
  logs: [],
  isLoading: false,
  error: null,
  lastLoadedAt: null,
  loadGlobalSearchData: async (signal?: AbortSignal) => {
    if (pendingLoad) {
      return pendingLoad;
    }

    pendingLoad = (async () => {
      set({ isLoading: true, error: null });

      const [tasksResult, worktreesResult, configResult, logsResult] = await Promise.allSettled([
        getRegistryTasks({ signal }),
        getWorktrees({ signal }),
        getConfig({ signal }),
        getLogs({ signal }),
      ]);

      const failedSources: Array<'tasks' | 'worktrees' | 'settings' | 'logs'> = [];
      const nextState = {
        tasks: [] as ApiRegistryTask[],
        worktrees: [] as ApiWorktree[],
        config: null as ApiConfigResponse | null,
        logs: [] as ApiLogFile[],
      };

      if (tasksResult.status === 'fulfilled') {
        nextState.tasks = tasksResult.value;
      } else {
        failedSources.push('tasks');
      }

      if (worktreesResult.status === 'fulfilled') {
        nextState.worktrees = worktreesResult.value;
      } else {
        failedSources.push('worktrees');
      }

      if (configResult.status === 'fulfilled') {
        nextState.config = configResult.value;
      } else {
        failedSources.push('settings');
      }

      if (logsResult.status === 'fulfilled') {
        nextState.logs = logsResult.value;
      } else {
        failedSources.push('logs');
      }

      set({
        ...nextState,
        error: failedSources.length > 0 ? formatLoadError(failedSources) : null,
        lastLoadedAt: Date.now(),
      });
    })().finally(() => {
      pendingLoad = null;
      set({ isLoading: false });
    });

    return pendingLoad;
  },
}));
