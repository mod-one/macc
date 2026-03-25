import React, { useState, useMemo, useEffect, useCallback, useRef } from 'react';
import { diffLines, type Change } from 'diff';
import { cn } from './styles';
import * as Icons from './icons';
import { getPrd } from '../api/client';
import type { ApiPrdTask, JsonValue } from '../api/models';

type DiffMode = 'unsaved-vs-saved' | 'project-vs-worktree';
type DiffLayout = 'unified' | 'split';

interface PrdDiffProps {
  /** Current (possibly unsaved) tasks in the editor */
  currentTasks: ApiPrdTask[];
  currentMetadata: Record<string, JsonValue>;
  /** Whether there are unsaved changes */
  hasUnsavedChanges: boolean;
}

function prdToText(tasks: ApiPrdTask[], metadata: Record<string, JsonValue>): string {
  return JSON.stringify({ metadata, tasks }, null, 2);
}

const PrdDiff: React.FC<PrdDiffProps> = ({ currentTasks, currentMetadata, hasUnsavedChanges }) => {
  const [mode, setMode] = useState<DiffMode>('unsaved-vs-saved');
  const [layout, setLayout] = useState<DiffLayout>('unified');
  const [savedTasks, setSavedTasks] = useState<ApiPrdTask[]>([]);
  const [savedMetadata, setSavedMetadata] = useState<Record<string, JsonValue>>({});
  const [worktreeTasks, setWorktreeTasks] = useState<ApiPrdTask[]>([]);
  const [worktreeMetadata, setWorktreeMetadata] = useState<Record<string, JsonValue>>({});
  const [projectTasks, setProjectTasks] = useState<ApiPrdTask[]>([]);
  const [projectMetadata, setProjectMetadata] = useState<Record<string, JsonValue>>({});
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [currentHunkIndex, setCurrentHunkIndex] = useState(0);

  const containerRef = useRef<HTMLDivElement>(null);
  const hunkRefs = useRef<Map<number, HTMLDivElement>>(new Map());

  // Fetch saved PRD for unsaved-vs-saved mode
  const fetchSavedPrd = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const data = await getPrd();
      setSavedTasks(data.tasks || []);
      setSavedMetadata(data.metadata || {});
    } catch (err) {
      setError('Failed to fetch saved PRD');
      console.error(err);
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Fetch both project and worktree PRDs for comparison
  const fetchProjectWorktreePrds = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const [projectData, worktreeData] = await Promise.all([
        getPrd({ path: 'project' }),
        getPrd({ path: 'worktree' }),
      ]);
      setProjectTasks(projectData.tasks || []);
      setProjectMetadata(projectData.metadata || {});
      setWorktreeTasks(worktreeData.tasks || []);
      setWorktreeMetadata(worktreeData.metadata || {});
    } catch (err) {
      setError('Failed to fetch PRDs for comparison');
      console.error(err);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    if (mode === 'unsaved-vs-saved') {
      fetchSavedPrd();
    } else {
      fetchProjectWorktreePrds();
    }
  }, [mode, fetchSavedPrd, fetchProjectWorktreePrds]);

  const { oldText, newText, oldLabel, newLabel } = useMemo(() => {
    if (mode === 'unsaved-vs-saved') {
      return {
        oldText: prdToText(savedTasks, savedMetadata),
        newText: prdToText(currentTasks, currentMetadata),
        oldLabel: 'Saved',
        newLabel: 'Unsaved (editor)',
      };
    }
    return {
      oldText: prdToText(projectTasks, projectMetadata),
      newText: prdToText(worktreeTasks, worktreeMetadata),
      oldLabel: 'Project PRD',
      newLabel: 'Worktree PRD',
    };
  }, [mode, savedTasks, savedMetadata, currentTasks, currentMetadata, projectTasks, projectMetadata, worktreeTasks, worktreeMetadata]);

  const changes = useMemo(() => diffLines(oldText, newText), [oldText, newText]);

  // Identify hunks (groups of changes that contain additions or removals)
  const hunkIndices = useMemo(() => {
    const indices: number[] = [];
    let lineIndex = 0;
    for (const change of changes) {
      if (change.added || change.removed) {
        indices.push(lineIndex);
        // Skip consecutive changed lines to avoid duplicate hunks
        lineIndex += (change.count || 1);
        continue;
      }
      lineIndex += (change.count || 1);
    }
    // Deduplicate adjacent hunks
    const deduped: number[] = [];
    let lastHunkLine = -10;
    for (const idx of indices) {
      if (idx - lastHunkLine > 3) {
        deduped.push(idx);
      }
      lastHunkLine = idx;
    }
    return deduped;
  }, [changes]);

  const totalHunks = hunkIndices.length;

  const navigateToHunk = useCallback((index: number) => {
    if (index < 0 || index >= totalHunks) return;
    setCurrentHunkIndex(index);
    const el = hunkRefs.current.get(index);
    if (el) {
      el.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
  }, [totalHunks]);

  const hasChanges = changes.some(c => c.added || c.removed);

  // Register hunk element refs
  const registerHunkRef = useCallback((hunkIdx: number, el: HTMLDivElement | null) => {
    if (el) {
      hunkRefs.current.set(hunkIdx, el);
    } else {
      hunkRefs.current.delete(hunkIdx);
    }
  }, []);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full text-[var(--text-muted)]">
        <div className="flex flex-col items-center gap-3">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-[var(--text-muted)] border-t-transparent" />
          <span className="text-sm">Loading PRD data...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      {/* Toolbar */}
      <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--bg-secondary)] px-4 py-2">
        <div className="flex items-center gap-3">
          {/* Mode selector */}
          <select
            value={mode}
            onChange={e => { setMode(e.target.value as DiffMode); setCurrentHunkIndex(0); }}
            className="h-8 rounded-lg border border-[var(--border)] bg-[var(--bg-card)] px-2 text-xs text-[var(--text-primary)] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]"
          >
            <option value="unsaved-vs-saved">Unsaved vs Saved</option>
            <option value="project-vs-worktree">Project vs Worktree</option>
          </select>

          {/* Layout toggle */}
          <div className="flex rounded-md border border-[var(--border)] overflow-hidden">
            <button
              onClick={() => setLayout('unified')}
              className={cn(
                "px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wide transition-colors",
                layout === 'unified' ? "bg-[var(--accent)] text-white" : "bg-[var(--bg-card)] text-[var(--text-muted)] hover:bg-[var(--bg-hover)]"
              )}
            >
              Unified
            </button>
            <button
              onClick={() => setLayout('split')}
              className={cn(
                "px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wide transition-colors",
                layout === 'split' ? "bg-[var(--accent)] text-white" : "bg-[var(--bg-card)] text-[var(--text-muted)] hover:bg-[var(--bg-hover)]"
              )}
            >
              Split
            </button>
          </div>
        </div>

        {/* Hunk navigation */}
        <div className="flex items-center gap-2">
          {totalHunks > 0 && (
            <span className="text-xs text-[var(--text-muted)]">
              {totalHunks} {totalHunks === 1 ? 'change' : 'changes'}
            </span>
          )}
          <button
            onClick={() => navigateToHunk(currentHunkIndex - 1)}
            disabled={currentHunkIndex <= 0 || totalHunks === 0}
            className="flex h-7 w-7 items-center justify-center rounded-md border border-[var(--border)] bg-[var(--bg-card)] text-[var(--text-muted)] transition-colors hover:bg-[var(--bg-hover)] disabled:opacity-30"
            title="Previous change"
          >
            <Icons.ChevronUpIcon className="h-3.5 w-3.5" />
          </button>
          <button
            onClick={() => navigateToHunk(currentHunkIndex + 1)}
            disabled={currentHunkIndex >= totalHunks - 1 || totalHunks === 0}
            className="flex h-7 w-7 items-center justify-center rounded-md border border-[var(--border)] bg-[var(--bg-card)] text-[var(--text-muted)] transition-colors hover:bg-[var(--bg-hover)] disabled:opacity-30"
            title="Next change"
          >
            <Icons.ChevronDownIcon className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>

      {/* Error banner */}
      {error && (
        <div className="border-b border-rose-500/30 bg-rose-500/10 px-4 py-2 text-xs text-rose-400">
          {error}
        </div>
      )}

      {/* Diff labels */}
      {layout === 'split' && (
        <div className="flex border-b border-[var(--border)] bg-[var(--bg-secondary)]">
          <div className="flex-1 px-4 py-1.5 text-[10px] font-bold uppercase tracking-wider text-[var(--text-muted)] border-r border-[var(--border)]">
            {oldLabel}
          </div>
          <div className="flex-1 px-4 py-1.5 text-[10px] font-bold uppercase tracking-wider text-[var(--text-muted)]">
            {newLabel}
          </div>
        </div>
      )}

      {/* Diff content */}
      <div ref={containerRef} className="flex-1 overflow-auto">
        {!hasChanges ? (
          <div className="flex flex-col items-center justify-center h-full text-[var(--text-muted)] py-20">
            <Icons.CheckIcon className="h-8 w-8 opacity-30 mb-3" />
            <p className="text-sm font-medium">No differences found</p>
            <p className="text-xs mt-1 opacity-60">
              {mode === 'unsaved-vs-saved'
                ? hasUnsavedChanges ? 'Editor changes match the saved PRD' : 'No unsaved changes'
                : 'Project and worktree PRDs are identical'}
            </p>
          </div>
        ) : layout === 'unified' ? (
          <UnifiedDiff changes={changes} hunkIndices={hunkIndices} registerHunkRef={registerHunkRef} />
        ) : (
          <SplitDiff changes={changes} hunkIndices={hunkIndices} registerHunkRef={registerHunkRef} />
        )}
      </div>
    </div>
  );
};

/* ─── Unified diff view ─── */

interface DiffViewProps {
  changes: Change[];
  hunkIndices: number[];
  registerHunkRef: (hunkIdx: number, el: HTMLDivElement | null) => void;
}

const UnifiedDiff: React.FC<DiffViewProps> = ({ changes, hunkIndices, registerHunkRef }) => {
  let oldLine = 1;
  let newLine = 1;
  let globalLine = 0;
  let nextHunkIdx = 0;

  return (
    <div className="font-mono text-xs leading-5">
      {changes.map((change, i) => {
        const lines = (change.value || '').replace(/\n$/, '').split('\n');
        const startGlobal = globalLine;

        return lines.map((line, j) => {
          const currentGlobal = startGlobal + j;
          let hunkRefIdx: number | null = null;

          // Check if this line starts a hunk
          if (nextHunkIdx < hunkIndices.length && currentGlobal === hunkIndices[nextHunkIdx]) {
            hunkRefIdx = nextHunkIdx;
            nextHunkIdx++;
          }

          let lineNum1 = '';
          let lineNum2 = '';
          let prefix = ' ';
          let bgClass = '';
          let textClass = 'text-[var(--text-primary)]';

          if (change.removed) {
            lineNum1 = String(oldLine++);
            prefix = '-';
            bgClass = 'bg-red-500/10';
            textClass = 'text-red-400';
            globalLine++;
          } else if (change.added) {
            lineNum2 = String(newLine++);
            prefix = '+';
            bgClass = 'bg-green-500/10';
            textClass = 'text-green-400';
            globalLine++;
          } else {
            lineNum1 = String(oldLine++);
            lineNum2 = String(newLine++);
            globalLine++;
          }

          return (
            <div
              key={`${i}-${j}`}
              ref={hunkRefIdx !== null ? (el) => registerHunkRef(hunkRefIdx, el) : undefined}
              className={cn("flex", bgClass)}
            >
              <span className="inline-block w-12 shrink-0 select-none text-right pr-2 text-[var(--text-muted)]/50">{lineNum1}</span>
              <span className="inline-block w-12 shrink-0 select-none text-right pr-2 text-[var(--text-muted)]/50">{lineNum2}</span>
              <span className={cn("inline-block w-4 shrink-0 select-none text-center", textClass)}>{prefix}</span>
              <span className={cn("flex-1 whitespace-pre-wrap break-all pr-4", textClass)}>{line}</span>
            </div>
          );
        });
      })}
    </div>
  );
};

/* ─── Split diff view ─── */

const SplitDiff: React.FC<DiffViewProps> = ({ changes, hunkIndices, registerHunkRef }) => {
  // Build left and right line arrays
  const leftLines: Array<{ num: number; text: string; type: 'removed' | 'context' | 'empty' }> = [];
  const rightLines: Array<{ num: number; text: string; type: 'added' | 'context' | 'empty' }> = [];

  let oldLine = 1;
  let newLine = 1;

  for (let ci = 0; ci < changes.length; ci++) {
    const change = changes[ci];
    const lines = (change.value || '').replace(/\n$/, '').split('\n');

    if (change.removed) {
      // Look ahead for a paired addition
      const next = ci + 1 < changes.length ? changes[ci + 1] : null;
      if (next && next.added) {
        const nextLines = (next.value || '').replace(/\n$/, '').split('\n');
        const maxLen = Math.max(lines.length, nextLines.length);
        for (let j = 0; j < maxLen; j++) {
          if (j < lines.length) {
            leftLines.push({ num: oldLine++, text: lines[j], type: 'removed' });
          } else {
            leftLines.push({ num: 0, text: '', type: 'empty' });
          }
          if (j < nextLines.length) {
            rightLines.push({ num: newLine++, text: nextLines[j], type: 'added' });
          } else {
            rightLines.push({ num: 0, text: '', type: 'empty' });
          }
        }
        ci++; // Skip the next (added) change since we consumed it
      } else {
        for (const line of lines) {
          leftLines.push({ num: oldLine++, text: line, type: 'removed' });
          rightLines.push({ num: 0, text: '', type: 'empty' });
        }
      }
    } else if (change.added) {
      for (const line of lines) {
        leftLines.push({ num: 0, text: '', type: 'empty' });
        rightLines.push({ num: newLine++, text: line, type: 'added' });
      }
    } else {
      for (const line of lines) {
        leftLines.push({ num: oldLine++, text: line, type: 'context' });
        rightLines.push({ num: newLine++, text: line, type: 'context' });
      }
    }
  }

  // Map hunk indices to row positions in the split view
  let globalLine = 0;
  let nextHunkIdx = 0;
  const hunkRowMap = new Map<number, number>();

  for (let ci = 0; ci < changes.length; ci++) {
    const change = changes[ci];
    const count = (change.value || '').replace(/\n$/, '').split('\n').length;
    for (let j = 0; j < count; j++) {
      if (nextHunkIdx < hunkIndices.length && globalLine === hunkIndices[nextHunkIdx]) {
        hunkRowMap.set(nextHunkIdx, leftLines.findIndex((_, idx) => {
          // Find the row index that corresponds to this global line
          let gl = 0;
          for (let k = 0; k <= idx; k++) {
            if (leftLines[k].type !== 'empty' || rightLines[k].type !== 'empty') gl++;
          }
          return gl >= globalLine;
        }));
        nextHunkIdx++;
      }
      globalLine++;
    }
  }

  const lineStyle = (type: string) => {
    switch (type) {
      case 'removed': return 'bg-red-500/10 text-red-400';
      case 'added': return 'bg-green-500/10 text-green-400';
      case 'empty': return 'bg-[var(--bg-secondary)]/50 text-transparent';
      default: return 'text-[var(--text-primary)]';
    }
  };

  return (
    <div className="flex font-mono text-xs leading-5">
      {/* Left pane */}
      <div className="flex-1 border-r border-[var(--border)]">
        {leftLines.map((line, i) => {
          const hunkIdx = [...hunkRowMap.entries()].find(([, row]) => row === i)?.[0];
          return (
            <div
              key={i}
              ref={hunkIdx !== undefined ? (el) => registerHunkRef(hunkIdx, el) : undefined}
              className={cn("flex", lineStyle(line.type))}
            >
              <span className="inline-block w-12 shrink-0 select-none text-right pr-2 text-[var(--text-muted)]/50">
                {line.num || ''}
              </span>
              <span className="inline-block w-4 shrink-0 select-none text-center">
                {line.type === 'removed' ? '-' : ' '}
              </span>
              <span className="flex-1 whitespace-pre-wrap break-all pr-2">{line.text}</span>
            </div>
          );
        })}
      </div>
      {/* Right pane */}
      <div className="flex-1">
        {rightLines.map((line, i) => (
          <div key={i} className={cn("flex", lineStyle(line.type))}>
            <span className="inline-block w-12 shrink-0 select-none text-right pr-2 text-[var(--text-muted)]/50">
              {line.num || ''}
            </span>
            <span className="inline-block w-4 shrink-0 select-none text-center">
              {line.type === 'added' ? '+' : ' '}
            </span>
            <span className="flex-1 whitespace-pre-wrap break-all pr-2">{line.text}</span>
          </div>
        ))}
      </div>
    </div>
  );
};

export default PrdDiff;
