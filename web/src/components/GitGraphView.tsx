import React, { useEffect, useMemo, useState } from 'react';
import { Link } from 'react-router-dom';
import { GitLog, type Commit, type GitLogEntry } from '@tomplum/react-git-log';
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

interface GitLogCommitMeta {
  shortSha: string;
  subject: string;
  timestamp: number;
  branchRefs: string[];
  taskId?: string;
}

const GRAPH_COLOURS = ['#3b82f6', '#14b8a6', '#f59e0b', '#ef4444', '#8b5cf6', '#22c55e'] as const;

function formatTimestamp(timestamp: number): string {
  return new Date(timestamp * 1000).toLocaleString();
}

function toIsoTimestamp(timestamp: number): string {
  return new Date(timestamp * 1000).toISOString();
}

function preferredBranch(commit: GitCommit, head: string): string {
  if (commit.branchRefs.includes(head)) {
    return head;
  }

  return commit.branchRefs[0] ?? head ?? 'detached';
}

function toGitLogEntry(commit: GitCommit, head: string): GitLogEntry<GitLogCommitMeta> {
  const isoTimestamp = toIsoTimestamp(commit.timestamp);

  return {
    hash: commit.sha,
    branch: preferredBranch(commit, head),
    parents: commit.parentShas,
    message: commit.subject,
    author: {
      name: commit.author,
    },
    authorDate: isoTimestamp,
    committerDate: isoTimestamp,
    shortSha: commit.shortSha,
    subject: commit.subject,
    timestamp: commit.timestamp,
    branchRefs: commit.branchRefs,
    taskId: commit.taskId,
  };
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

  const entries = useMemo(
    () => commits.map((commit) => toGitLogEntry(commit, head || 'detached')),
    [commits, head],
  );
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

  const handleSelectCommit = (commit?: Commit<GitLogCommitMeta>) => {
    setSelectedSha(commit?.hash ?? null);
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

      <div className="min-h-0 flex-1 overflow-auto rounded border border-[var(--border)] bg-[var(--bg-secondary)] p-2">
        {isLoading && commits.length === 0 ? (
          <div className="flex h-full items-center justify-center text-sm text-[var(--text-muted)]">
            Loading git graph...
          </div>
        ) : commits.length === 0 ? (
          <div className="flex h-full items-center justify-center text-sm text-[var(--text-muted)]">
            No commits yet.
          </div>
        ) : (
          <div className="space-y-3">
            <GitLog<GitLogCommitMeta>
              entries={entries}
              currentBranch={head || 'detached'}
              colours={[...GRAPH_COLOURS]}
              theme="dark"
              showHeaders={mode === 'page'}
              rowSpacing={mode === 'page' ? 2 : 0}
              defaultGraphWidth={mode === 'page' ? 240 : 180}
              onSelectCommit={handleSelectCommit}
              enableSelectedCommitStyling
              enablePreviewedCommitStyling
            >
              <GitLog.Tags />
              <GitLog.GraphHTMLGrid
                enableResize={mode === 'page'}
                nodeTheme="plain"
                showCommitNodeTooltips
                showCommitNodeHashes={mode === 'page'}
                nodeSize={mode === 'page' ? 14 : 12}
                highlightedBackgroundHeight={mode === 'page' ? 56 : 48}
              />
              <GitLog.Table timestampFormat="YYYY-MM-DD HH:mm:ss" />
            </GitLog>

            {hasMore && (
              <button
                type="button"
                onClick={handleLoadMore}
                disabled={isLoadingMore}
                className="w-full rounded border border-[var(--border)] bg-[var(--bg-card)] px-3 py-2 text-sm text-[var(--text-primary)] transition-colors hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {isLoadingMore ? 'Loading older commits...' : 'Load older commits'}
              </button>
            )}
          </div>
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
