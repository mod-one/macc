import React, { useEffect, useId, useMemo, useState } from 'react';
import { Link } from 'react-router-dom';
import {
  CommitGraph,
  type Branch as GraphBranch,
  type Commit as GraphCommit,
  type CommitNode,
} from 'commit-graph';
import type { GitCommit } from '../api/models';
import {
  DEFAULT_PAGE_LIMIT,
  DEFAULT_PANEL_LIMIT,
  useGitGraphStore,
} from '../stores/gitGraphStore';

type GitGraphViewMode = 'panel' | 'page';

interface GitGraphViewProps {
  mode: GitGraphViewMode;
}

const GRAPH_STYLE_BY_MODE = {
  panel: {
    commitSpacing: 60,
    branchSpacing: 26,
    nodeRadius: 3,
    branchColors: ['#3b82f6', '#14b8a6', '#f59e0b', '#ef4444', '#8b5cf6', '#22c55e'],
  },
  page: {
    commitSpacing: 72,
    branchSpacing: 30,
    nodeRadius: 3,
    branchColors: ['#3b82f6', '#14b8a6', '#f59e0b', '#ef4444', '#8b5cf6', '#22c55e'],
  },
} as const;

function formatTimestamp(timestamp: number): string {
  return new Date(timestamp * 1000).toLocaleString();
}

function toGraphCommit(commit: GitCommit): GraphCommit {
  return {
    sha: commit.sha,
    commit: {
      author: {
        name: commit.author,
        date: new Date(commit.timestamp * 1000),
      },
      message: commit.taskId ? `${commit.subject} [${commit.taskId}]` : commit.subject,
    },
    parents: commit.parentShas.map((sha) => ({ sha })),
  };
}

function toBranchHeads(commits: GitCommit[], branches: string[]): GraphBranch[] {
  return branches
    .map((name) => {
      const first = commits.find((commit) => commit.branchRefs.includes(name));
      if (!first) {
        return null;
      }
      return {
        name,
        commit: {
          sha: first.sha,
        },
      };
    })
    .filter((branch): branch is GraphBranch => branch !== null);
}

const GitGraphView: React.FC<GitGraphViewProps> = ({ mode }) => {
  const commits = useGitGraphStore((state) => state.commits);
  const branches = useGitGraphStore((state) => state.branches);
  const head = useGitGraphStore((state) => state.head);
  const isLoading = useGitGraphStore((state) => state.isLoading);
  const isLoadingMore = useGitGraphStore((state) => state.isLoadingMore);
  const error = useGitGraphStore((state) => state.error);
  const hasMore = useGitGraphStore((state) => state.hasMore);
  const loadGraph = useGitGraphStore((state) => state.loadGraph);
  const loadMore = useGitGraphStore((state) => state.loadMore);
  const refreshLatest = useGitGraphStore((state) => state.refreshLatest);

  const [selectedSha, setSelectedSha] = useState<string | null>(null);
  const graphParentId = useId().replace(/[:]/g, '_');
  const initialLimit = mode === 'page' ? DEFAULT_PAGE_LIMIT : DEFAULT_PANEL_LIMIT;

  useEffect(() => {
    if (commits.length === 0 || (mode === 'page' && commits.length < DEFAULT_PAGE_LIMIT)) {
      void loadGraph(initialLimit);
    }
  }, [commits.length, initialLimit, loadGraph, mode]);

  useEffect(() => {
    const interval = window.setInterval(() => {
      void refreshLatest(DEFAULT_PANEL_LIMIT);
    }, 30_000);

    return () => {
      window.clearInterval(interval);
    };
  }, [refreshLatest]);

  const graphCommits = useMemo(() => commits.map(toGraphCommit), [commits]);
  const branchHeads = useMemo(() => toBranchHeads(commits, branches), [branches, commits]);
  const selectedCommit = useMemo(
    () => (selectedSha ? commits.find((commit) => commit.sha === selectedSha) ?? null : null),
    [commits, selectedSha],
  );
  const taskLinkedCommits = useMemo(
    () => commits.filter((commit) => Boolean(commit.taskId)).slice(0, mode === 'page' ? 16 : 8),
    [commits, mode],
  );

  const handleLoadMore = () => {
    void loadMore(initialLimit);
  };

  const onCommitClick = (node: CommitNode) => {
    setSelectedSha(node.hash);
  };

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2 text-xs text-[var(--text-secondary)]">
        <span className="rounded border border-[var(--border)] bg-[var(--bg-card)] px-2 py-1 font-mono text-[var(--text-primary)]">
          HEAD: {head || 'detached'}
        </span>
        {branches.slice(0, mode === 'page' ? 12 : 6).map((branch) => (
          <span
            key={branch}
            className="rounded border border-[var(--border)] bg-[var(--bg-secondary)] px-2 py-1"
            title={branch}
          >
            {branch}
          </span>
        ))}
        {branches.length > (mode === 'page' ? 12 : 6) && (
          <span className="text-[var(--text-muted)]">
            +{branches.length - (mode === 'page' ? 12 : 6)} branches
          </span>
        )}
      </div>

      {error && (
        <div className="rounded border border-[var(--error)]/50 bg-[var(--error)]/10 px-3 py-2 text-sm text-[var(--error)]">
          {error}
        </div>
      )}

      <div
        id={graphParentId}
        className="min-h-0 flex-1 overflow-auto rounded border border-[var(--border)] bg-[var(--bg-secondary)] p-2"
      >
        {isLoading && commits.length === 0 ? (
          <div className="flex h-full items-center justify-center text-sm text-[var(--text-muted)]">
            Loading git graph...
          </div>
        ) : commits.length === 0 ? (
          <div className="flex h-full items-center justify-center text-sm text-[var(--text-muted)]">
            No commits yet.
          </div>
        ) : (
          <CommitGraph.WithInfiniteScroll
            parentID={graphParentId}
            commits={graphCommits}
            branchHeads={branchHeads}
            loadMore={handleLoadMore}
            hasMore={hasMore}
            currentBranch={head}
            graphStyle={GRAPH_STYLE_BY_MODE[mode]}
            fullSha={mode === 'page'}
            onCommitClick={onCommitClick}
            dateFormatFn={(value: string | number | Date) => new Date(value).toLocaleString()}
          />
        )}
      </div>

      <div className="rounded border border-[var(--border)] bg-[var(--bg-secondary)] p-3 text-sm">
        <div className="mb-2 flex items-center justify-between text-xs text-[var(--text-secondary)]">
          <span>{selectedCommit ? 'Commit Details' : 'Click a commit for details'}</span>
          {isLoadingMore && <span className="text-[var(--accent)]">Loading older commits...</span>}
        </div>

        {selectedCommit ? (
          <div className="space-y-2">
            <div className="font-mono text-xs text-[var(--text-primary)]">{selectedCommit.sha}</div>
            <div className="text-[var(--text-primary)]">{selectedCommit.subject}</div>
            <div className="text-xs text-[var(--text-secondary)]">
              {selectedCommit.author} · {formatTimestamp(selectedCommit.timestamp)}
            </div>
            {selectedCommit.taskId && (
              <Link
                to={`/ops/registry?task=${encodeURIComponent(selectedCommit.taskId)}`}
                className="inline-flex rounded-full border border-[var(--accent)] bg-[var(--accent)]/15 px-2 py-1 font-mono text-xs text-[var(--accent)]"
              >
                {selectedCommit.taskId}
              </Link>
            )}
          </div>
        ) : (
          <div className="text-xs text-[var(--text-muted)]">
            Select a commit to inspect SHA, message, author, date, and task linkage.
          </div>
        )}
      </div>

      {taskLinkedCommits.length > 0 && (
        <div className="rounded border border-[var(--border)] bg-[var(--bg-secondary)] p-3">
          <div className="mb-2 text-xs text-[var(--text-secondary)]">Task-linked commits</div>
          <div className="flex flex-wrap gap-2">
            {taskLinkedCommits.map((commit) => (
              <button
                key={commit.sha}
                type="button"
                onClick={() => setSelectedSha(commit.sha)}
                className="inline-flex items-center gap-2 rounded border border-[var(--border)] bg-[var(--bg-card)] px-2 py-1 text-xs"
              >
                <span className="font-mono text-[var(--text-primary)]">{commit.shortSha}</span>
                <span className="rounded-full border border-[var(--accent)] bg-[var(--accent)]/15 px-2 py-0.5 font-mono text-[var(--accent)]">
                  {commit.taskId}
                </span>
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
};

export default GitGraphView;
