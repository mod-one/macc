import { create } from 'zustand';
import { getGitGraph } from '../api/client';
import type { GitCommit } from '../api/models';

const DEFAULT_PANEL_LIMIT = 100;
const DEFAULT_PAGE_LIMIT = 220;

interface GitGraphStoreState {
  commits: GitCommit[];
  branches: string[];
  head: string;
  isLoading: boolean;
  isLoadingMore: boolean;
  error: string | null;
  hasMore: boolean;
}

interface GitGraphStoreActions {
  loadGraph: (limit?: number) => Promise<void>;
  loadMore: (limit?: number) => Promise<void>;
  refreshLatest: (limit?: number) => Promise<void>;
}

type GitGraphStore = GitGraphStoreState & GitGraphStoreActions;

const initialState: GitGraphStoreState = {
  commits: [],
  branches: [],
  head: '',
  isLoading: false,
  isLoadingMore: false,
  error: null,
  hasMore: true,
};

function uniqueBySha(commits: GitCommit[]): GitCommit[] {
  const seen = new Set<string>();
  const merged: GitCommit[] = [];

  for (const commit of commits) {
    if (seen.has(commit.sha)) {
      continue;
    }
    seen.add(commit.sha);
    merged.push(commit);
  }

  return merged;
}

export const useGitGraphStore = create<GitGraphStore>((set, get) => ({
  ...initialState,

  async loadGraph(limit = DEFAULT_PANEL_LIMIT) {
    set({ isLoading: true, error: null });

    try {
      const response = await getGitGraph({ limit });
      set({
        commits: response.commits,
        branches: response.branches,
        head: response.head,
        hasMore: response.commits.length >= limit,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to load git graph.';
      set({ error: message });
      throw error;
    } finally {
      set({ isLoading: false });
    }
  },

  async loadMore(limit = DEFAULT_PANEL_LIMIT) {
    const { commits, isLoadingMore, hasMore } = get();
    if (!hasMore || isLoadingMore || commits.length === 0) {
      return;
    }

    const cursor = commits[commits.length - 1]?.sha;
    if (!cursor) {
      return;
    }

    set({ isLoadingMore: true, error: null });

    try {
      const response = await getGitGraph({ limit, since: cursor });
      set((state) => {
        const merged = uniqueBySha(state.commits.concat(response.commits));
        return {
          commits: merged,
          branches: response.branches,
          head: response.head,
          hasMore: response.commits.length >= limit,
        };
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to load older commits.';
      set({ error: message });
      throw error;
    } finally {
      set({ isLoadingMore: false });
    }
  },

  async refreshLatest(limit = DEFAULT_PANEL_LIMIT) {
    const { commits } = get();
    if (commits.length === 0) {
      await get().loadGraph(limit);
      return;
    }

    try {
      const response = await getGitGraph({ limit });
      const known = new Set(commits.map((commit) => commit.sha));
      const fresh = response.commits.filter((commit) => !known.has(commit.sha));

      if (fresh.length === 0) {
        set({ branches: response.branches, head: response.head });
        return;
      }

      set((state) => ({
        commits: uniqueBySha(fresh.concat(state.commits)),
        branches: response.branches,
        head: response.head,
      }));
    } catch {
      // Keep graph usable even if background polling fails.
    }
  },
}));

export { DEFAULT_PAGE_LIMIT, DEFAULT_PANEL_LIMIT };
