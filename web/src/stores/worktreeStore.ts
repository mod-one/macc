import { create } from 'zustand';
import { getWorktrees, deleteWorktree, runWorktree } from '../api/client';
import type { ApiWorktree } from '../api/models';

interface WorktreeState {
  worktrees: ApiWorktree[];
  isLoading: boolean;
  error: string | null;
}

interface WorktreeActions {
  loadWorktrees: () => Promise<void>;
  removeWorktree: (id: string) => Promise<void>;
  runWorktree: (id: string) => Promise<void>;
}

export const useWorktreeStore = create<WorktreeState & WorktreeActions>((set) => ({
  worktrees: [],
  isLoading: false,
  error: null,

  loadWorktrees: async () => {
    set({ isLoading: true });
    try {
      const worktrees = await getWorktrees();
      set({ worktrees, error: null });
    } catch (err) {
      set({ error: err instanceof Error ? err.message : 'Failed to load worktrees' });
    } finally {
      set({ isLoading: false });
    }
  },

  removeWorktree: async (id: string) => {
    try {
      await deleteWorktree(id, { confirmed: true });
      set((state) => ({
        worktrees: state.worktrees.filter((w) => w.id !== id),
      }));
    } catch (err) {
      set({ error: err instanceof Error ? err.message : 'Failed to remove worktree' });
      throw err;
    }
  },

  runWorktree: async (id: string) => {
    try {
      await runWorktree(id);
      // Optionally refresh after starting a run
    } catch (err) {
      set({ error: err instanceof Error ? err.message : 'Failed to run worktree' });
      throw err;
    }
  },
}));
