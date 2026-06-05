import React, { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import {
  useReactTable,
  getCoreRowModel,
  getSortedRowModel,
  getFilteredRowModel,
  flexRender,
  createColumnHelper,
  type SortingState,
} from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { getConfig, getPrd, updatePrd, sendJson } from '../api/client';
import type { ApiPrdTask, JsonValue } from '../api/models';
import { Button } from '../components/Button';
import * as Icons from '../components/icons';
import { cn } from '../components/styles';
import PrdGraph from '../components/PrdGraph';
import PrdDiff from '../components/PrdDiff';

// ── PRD generation API types ──────────────────────────────────────────────────

interface ApiPrdGenerateRequest {
  fromPath: string;
  tool?: string;
  instructions?: string;
  dryRun?: boolean;
  promote?: boolean;
}

interface ApiPrdGenerateResponse {
  status: string;
  runId?: string;
  targetDir?: string;
  tool?: string;
  prompt?: string;
}

interface ApiPrdAuditRequest {
  prdPath?: string;
  tool?: string;
  instructions?: string;
  referenceBranch?: string;
  diffStat?: boolean;
  dryRun?: boolean;
}

interface ApiPrdAuditResponse {
  completedWithContext: number;
  todoTasks: number;
  promptGenerated: boolean;
  prompt?: string;
  dispatched: boolean;
}

interface ApiPrdPromoteRequest {
  sourcePath: string;
  destPath?: string;
  yes?: boolean;
}

interface ApiPrdPromoteResult {
  promoted: boolean;
  sourcePath: string;
  destPath: string;
  backedUpExisting: boolean;
}

interface ApiPrdRunEntry {
  runId: string;
  path: string;
}

// ── API helpers ───────────────────────────────────────────────────────────────

function prdGenerate(req: ApiPrdGenerateRequest) {
  return sendJson<ApiPrdGenerateResponse, ApiPrdGenerateRequest>('/prd/generate', 'POST', {}, req);
}

function prdAudit(req: ApiPrdAuditRequest) {
  return sendJson<ApiPrdAuditResponse, ApiPrdAuditRequest>('/prd/audit', 'POST', {}, req);
}

function prdPromote(req: ApiPrdPromoteRequest) {
  return sendJson<ApiPrdPromoteResult, ApiPrdPromoteRequest>('/prd/promote', 'POST', {}, req);
}

function prdListRuns() {
  return sendJson<ApiPrdRunEntry[]>('/prd/generation-runs', 'GET', {});
}

// ── Types ─────────────────────────────────────────────────────────────────────

type PrdViewMode = 'table' | 'graph' | 'diff';
type SidePanel = 'task' | 'generate' | 'audit' | 'promote' | 'runs' | null;

const columnHelper = createColumnHelper<ApiPrdTask>();

// ── Status badge ──────────────────────────────────────────────────────────────

function statusColor(state: string): string {
  switch (state?.toLowerCase()) {
    case 'merged': return 'var(--success)';
    case 'in_progress': case 'claimed': return 'var(--accent)';
    case 'blocked': case 'failed': return 'var(--error)';
    case 'todo': case 'queued': return 'var(--text-muted)';
    case 'reviewing': case 'changes_requested': return 'var(--warning)';
    default: return 'var(--text-muted)';
  }
}

// ── Main component ────────────────────────────────────────────────────────────

const PrdPage: React.FC = () => {
  // PRD data
  const [tasks, setTasks] = useState<ApiPrdTask[]>([]);
  const [metadata, setMetadata] = useState<Record<string, JsonValue>>({});
  const [prdFilePath, setPrdFilePath] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  // Edit state
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [unsavedChanges, setUnsavedChanges] = useState(false);
  const [validationErrors, setValidationErrors] = useState<string[]>([]);

  // View
  const [globalFilter, setGlobalFilter] = useState('');
  const [sorting, setSorting] = useState<SortingState>([]);
  const [isJsonMode, setIsJsonMode] = useState(false);
  const [viewMode, setViewMode] = useState<PrdViewMode>(() => {
    try { return (localStorage.getItem('prd-view-mode') as PrdViewMode) || 'table'; }
    catch { return 'table'; }
  });
  const [sidePanel, setSidePanel] = useState<SidePanel>(null);

  const tableContainerRef = useRef<HTMLDivElement>(null);

  // ── Load: config → prdFile path → PRD data ─────────────────────────────────

  const loadPrd = useCallback(async (path: string | null) => {
    setIsLoading(true);
    setLoadError(null);
    try {
      const data = await getPrd(path ? { path } : {});
      setTasks(data.tasks ?? []);
      setMetadata(data.metadata ?? {});
      setUnsavedChanges(false);
      setValidationErrors([]);
    } catch (err) {
      setLoadError(err instanceof Error ? err.message : 'Failed to load PRD.');
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    void (async () => {
      // Read prdFile from config; fall back to default prd.json
      let path: string | null = null;
      try {
        const cfg = await getConfig();
        path = cfg.prdFile ?? null;
      } catch {
        // ignore — use default path
      }
      setPrdFilePath(path);
      await loadPrd(path);
    })();
  }, [loadPrd]);

  // ── View mode ───────────────────────────────────────────────────────────────

  const handleViewModeChange = (mode: PrdViewMode) => {
    setViewMode(mode);
    try { localStorage.setItem('prd-view-mode', mode); } catch { /* noop */ }
  };

  // ── Table ───────────────────────────────────────────────────────────────────

  const selectedTask = useMemo(
    () => tasks.find((t) => t.id === selectedTaskId) ?? null,
    [tasks, selectedTaskId],
  );

  const columns = useMemo(
    () => [
      columnHelper.accessor('id', {
        header: 'ID',
        cell: (info) => <span className="font-mono text-[11px]">{info.getValue()}</span>,
        size: 160,
      }),
      columnHelper.accessor('title', {
        header: 'Title',
        cell: (info) => <span className="font-medium">{info.getValue() ?? '(No title)'}</span>,
        size: 280,
      }),
      columnHelper.accessor('state' as keyof ApiPrdTask, {
        header: 'State',
        cell: (info) => {
          const v = String(info.getValue() ?? '');
          return (
            <span className="flex items-center gap-1.5 text-[11px]">
              <span
                className="h-1.5 w-1.5 rounded-full"
                style={{ backgroundColor: statusColor(v) }}
              />
              <span style={{ color: statusColor(v) }}>{v || 'todo'}</span>
            </span>
          );
        },
        size: 110,
      }),
      columnHelper.accessor('category', {
        header: 'Category',
        size: 110,
      }),
      columnHelper.accessor('priority', {
        header: 'Prio',
        size: 70,
      }),
      columnHelper.accessor('dependencies', {
        header: 'Deps',
        cell: (info) => (info.getValue() ?? []).length,
        size: 70,
      }),
    ],
    [],
  );

  const table = useReactTable({
    data: tasks,
    columns,
    state: { sorting, globalFilter },
    onSortingChange: setSorting,
    onGlobalFilterChange: setGlobalFilter,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
  });

  const { rows } = table.getRowModel();

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => tableContainerRef.current,
    estimateSize: () => 40,
    overscan: 10,
  });

  // ── Edit helpers ────────────────────────────────────────────────────────────

  const handleTaskUpdate = (updatedTask: ApiPrdTask) => {
    setTasks((prev) => prev.map((t) => (t.id === updatedTask.id ? updatedTask : t)));
    setUnsavedChanges(true);
  };

  const validatePrd = () => {
    const errors: string[] = [];
    const taskIds = new Set(tasks.map((t) => t.id));
    for (const task of tasks) {
      if (!task.id) { errors.push('Task with missing ID found.'); continue; }
      if (!task.title) errors.push(`Task ${task.id} missing title.`);
      for (const depId of task.dependencies ?? []) {
        if (!taskIds.has(depId)) errors.push(`Task ${task.id}: unknown dependency ${depId}`);
      }
      if ((task.dependencies ?? []).includes(task.id)) {
        errors.push(`Task ${task.id} depends on itself.`);
      }
    }
    setValidationErrors(errors);
    return errors.length === 0;
  };

  const handleSave = async () => {
    if (!validatePrd()) return;
    setIsSaving(true);
    try {
      await updatePrd({ tasks, metadata }, prdFilePath ? { path: prdFilePath } : {});
      setUnsavedChanges(false);
      setValidationErrors([]);
    } catch (err) {
      setValidationErrors([err instanceof Error ? err.message : 'Save failed.']);
    } finally {
      setIsSaving(false);
    }
  };

  const handleDiscard = () => {
    if (window.confirm('Discard all unsaved changes?')) {
      void loadPrd(prdFilePath);
    }
  };

  // ── Side panel opener ───────────────────────────────────────────────────────

  const openPanel = (panel: SidePanel) => {
    setSelectedTaskId(null);
    setSidePanel(panel);
  };

  const handleTaskSelect = (id: string) => {
    setSidePanel('task');
    setSelectedTaskId(id);
  };

  // ── Render ──────────────────────────────────────────────────────────────────

  return (
    <div className="flex h-[calc(100vh-80px)] flex-col gap-3 overflow-hidden">
      {/* Header */}
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-3">
          <div>
            <h1 className="text-xl font-semibold text-[var(--text-primary)]">PRD</h1>
            {prdFilePath ? (
              <p className="font-mono text-[11px] text-[var(--text-muted)]">{prdFilePath}</p>
            ) : (
              <p className="text-[11px] text-[var(--text-muted)]">prd.json (default)</p>
            )}
          </div>
          <span className="rounded border border-[var(--border)] bg-[var(--bg-secondary)] px-2 py-0.5 text-[11px] text-[var(--text-muted)]">
            {tasks.length} tasks
          </span>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          {/* View toggle */}
          <div className="flex overflow-hidden rounded-md border border-[var(--border)] bg-[var(--bg-card)]">
            {(['table', 'graph', 'diff'] as PrdViewMode[]).map((mode) => (
              <button
                key={mode}
                onClick={() => handleViewModeChange(mode)}
                className={cn(
                  'px-3 py-1.5 text-xs font-medium capitalize transition-colors',
                  viewMode === mode
                    ? 'bg-[var(--accent)] text-white'
                    : 'text-[var(--text-secondary)] hover:bg-white/8',
                )}
              >
                {mode}
              </button>
            ))}
          </div>

          {viewMode === 'table' && (
            <div className="relative">
              <Icons.SearchIcon className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--text-muted)]" />
              <input
                type="text"
                value={globalFilter}
                onChange={(e) => setGlobalFilter(e.target.value)}
                placeholder="Search tasks"
                className="h-8 w-52 rounded-md border border-[var(--border)] bg-[var(--bg-card)] pl-8 pr-3 text-sm placeholder:text-[var(--text-muted)] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50"
              />
            </div>
          )}

          {/* PRD operation buttons */}
          <div className="flex items-center gap-1.5">
            <Button
              className="h-8 border-[var(--border)] bg-[var(--bg-card)] px-2.5 text-xs"
              onClick={() => openPanel('generate')}
              title="Generate PRD"
            >
              <Icons.SparklesIcon className="mr-1 h-3.5 w-3.5" />
              Generate
            </Button>
            <Button
              className="h-8 border-[var(--border)] bg-[var(--bg-card)] px-2.5 text-xs"
              onClick={() => openPanel('audit')}
              title="Audit PRD"
            >
              <Icons.SearchIcon className="mr-1 h-3.5 w-3.5" />
              Audit
            </Button>
            <Button
              className="h-8 border-[var(--border)] bg-[var(--bg-card)] px-2.5 text-xs"
              onClick={() => openPanel('promote')}
              title="Promote generated PRD"
            >
              <Icons.ArrowUpIcon className="mr-1 h-3.5 w-3.5" />
              Promote
            </Button>
            <Button
              className="h-8 border-[var(--border)] bg-[var(--bg-card)] px-2.5 text-xs"
              onClick={() => openPanel('runs')}
              title="Generation runs"
            >
              <Icons.LogsIcon className="mr-1 h-3.5 w-3.5" />
              Runs
            </Button>
          </div>

          {/* Save / discard */}
          {unsavedChanges && (
            <div className="flex items-center gap-1.5">
              <Button
                className="h-8 border-[var(--border)] bg-[var(--bg-card)] px-2.5 text-xs"
                onClick={handleDiscard}
                disabled={isSaving}
              >
                Discard
              </Button>
              <Button
                className="h-8 border-transparent bg-[var(--accent)] px-2.5 text-xs text-white hover:brightness-110"
                onClick={() => void handleSave()}
                disabled={isSaving}
              >
                {isSaving ? 'Saving...' : 'Save changes'}
              </Button>
            </div>
          )}

          <Button
            className="h-8 border-[var(--border)] bg-[var(--bg-card)] px-2.5 text-xs"
            onClick={() => void loadPrd(prdFilePath)}
            disabled={isLoading}
            title="Reload PRD"
          >
            <Icons.RefreshIcon className={cn('h-3.5 w-3.5', isLoading && 'animate-spin')} />
          </Button>
        </div>
      </header>

      {/* Errors */}
      {(validationErrors.length > 0 || loadError) && (
        <div className="rounded-md border border-[var(--error)]/40 bg-[var(--error)]/10 px-4 py-2.5 text-sm text-[var(--text-primary)]">
          {loadError ?? validationErrors.slice(0, 3).join(' · ')}
          {validationErrors.length > 3 && ` · +${validationErrors.length - 3} more`}
        </div>
      )}

      {/* Body */}
      <div className="flex min-h-0 flex-1 gap-3 overflow-hidden">
        {/* Main view */}
        {viewMode === 'table' ? (
          <div
            ref={tableContainerRef}
            className="flex-1 overflow-auto rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
            style={{ boxShadow: 'var(--shadow-soft)' }}
          >
            {isLoading ? (
              <div className="flex h-40 items-center justify-center text-sm text-[var(--text-muted)]">
                Loading…
              </div>
            ) : (
              <table className="w-full border-collapse text-left text-sm">
                <thead className="sticky top-0 z-10 border-b border-[var(--border)] bg-[var(--bg-secondary)]">
                  {table.getHeaderGroups().map((hg) => (
                    <tr key={hg.id}>
                      {hg.headers.map((h) => (
                        <th
                          key={h.id}
                          onClick={h.column.getToggleSortingHandler()}
                          className="cursor-pointer px-4 py-2.5 text-[11px] font-semibold uppercase tracking-wide text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors"
                          style={{ width: h.getSize() }}
                        >
                          <span className="flex items-center gap-1">
                            {flexRender(h.column.columnDef.header, h.getContext())}
                            {h.column.getIsSorted() === 'asc' && <Icons.ChevronUpIcon className="h-3 w-3" />}
                            {h.column.getIsSorted() === 'desc' && <Icons.ChevronDownIcon className="h-3 w-3" />}
                          </span>
                        </th>
                      ))}
                    </tr>
                  ))}
                </thead>
                <tbody
                  style={{ height: rowVirtualizer.getTotalSize(), position: 'relative' }}
                >
                  {rowVirtualizer.getVirtualItems().map((vr) => {
                    const row = rows[vr.index];
                    const isSelected = selectedTaskId === row.original.id;
                    return (
                      <tr
                        key={row.id}
                        data-index={vr.index}
                        ref={rowVirtualizer.measureElement}
                        onClick={() => handleTaskSelect(row.original.id)}
                        className={cn(
                          'absolute w-full cursor-pointer border-b border-[var(--border-subtle)] transition-colors hover:bg-[var(--bg-elevated)]',
                          isSelected && 'bg-[var(--accent-bg)] hover:bg-[var(--accent-bg-hover)]',
                        )}
                        style={{ transform: `translateY(${vr.start}px)` }}
                      >
                        {row.getVisibleCells().map((cell) => (
                          <td
                            key={cell.id}
                            className="px-4 py-2.5 align-middle"
                            style={{ width: cell.column.getSize() }}
                          >
                            {flexRender(cell.column.columnDef.cell, cell.getContext())}
                          </td>
                        ))}
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            )}
            {!isLoading && rows.length === 0 && (
              <div className="flex flex-col items-center justify-center py-16 text-[var(--text-muted)]">
                <Icons.FolderOpenIcon className="mb-3 h-8 w-8 opacity-20" />
                <p className="text-sm">No tasks.</p>
                <button
                  className="mt-2 text-xs hover:underline"
                  style={{ color: 'var(--accent)' }}
                  onClick={() => openPanel('generate')}
                >
                  Generate a PRD from a brief
                </button>
              </div>
            )}
          </div>
        ) : viewMode === 'graph' ? (
          <div className="flex-1 overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]">
            <PrdGraph tasks={tasks} onSelectTask={setSelectedTaskId} selectedTaskId={selectedTaskId} />
          </div>
        ) : (
          <div className="flex-1 overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]">
            <PrdDiff currentTasks={tasks} currentMetadata={metadata} hasUnsavedChanges={unsavedChanges} />
          </div>
        )}

        {/* Side panel */}
        {sidePanel && (
          <aside className="flex w-[400px] shrink-0 flex-col overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]" style={{ boxShadow: 'var(--shadow-soft)' }}>
            <header className="flex items-center justify-between border-b border-[var(--border)] px-4 py-2.5">
              <span className="text-sm font-semibold text-[var(--text-primary)]">
                {sidePanel === 'task' && (selectedTask?.id ?? 'Task')}
                {sidePanel === 'generate' && 'Generate PRD'}
                {sidePanel === 'audit' && 'Audit PRD'}
                {sidePanel === 'promote' && 'Promote PRD'}
                {sidePanel === 'runs' && 'Generation runs'}
              </span>
              <button
                onClick={() => { setSidePanel(null); setSelectedTaskId(null); }}
                className="rounded p-1 text-[var(--text-muted)] hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)] transition-colors"
              >
                <Icons.XIcon className="h-4 w-4" />
              </button>
            </header>

            <div className="flex-1 overflow-y-auto p-4">
              {sidePanel === 'task' && selectedTask && (
                <TaskPanel
                  task={selectedTask}
                  isJsonMode={isJsonMode}
                  onToggleJson={() => setIsJsonMode((v) => !v)}
                  onChange={handleTaskUpdate}
                />
              )}
              {sidePanel === 'generate' && (
                <GeneratePanel
                  onDone={() => void loadPrd(prdFilePath)}
                />
              )}
              {sidePanel === 'audit' && (
                <AuditPanel prdFilePath={prdFilePath} />
              )}
              {sidePanel === 'promote' && (
                <PromotePanel
                  onDone={() => void loadPrd(prdFilePath)}
                />
              )}
              {sidePanel === 'runs' && (
                <RunsPanel onPromote={() => setSidePanel('promote')} />
              )}
            </div>
          </aside>
        )}
      </div>
    </div>
  );
};

// ── Task panel ────────────────────────────────────────────────────────────────

interface TaskPanelProps {
  task: ApiPrdTask;
  isJsonMode: boolean;
  onToggleJson: () => void;
  onChange: (task: ApiPrdTask) => void;
}

const TaskPanel: React.FC<TaskPanelProps> = ({ task, isJsonMode, onToggleJson, onChange }) => {
  const handleChange = (field: keyof ApiPrdTask, value: JsonValue) => {
    onChange({ ...task, [field]: value } as ApiPrdTask);
  };

  return (
    <div className="space-y-4">
      <div className="flex justify-end">
        <button
          onClick={onToggleJson}
          className={cn(
            'rounded px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider transition-colors',
            isJsonMode ? 'bg-[var(--accent)] text-white' : 'border border-[var(--border)] bg-[var(--bg-secondary)] text-[var(--text-muted)]',
          )}
        >
          JSON
        </button>
      </div>

      {isJsonMode ? (
        <textarea
          value={JSON.stringify(task, null, 2)}
          onChange={(e) => {
            try { onChange(JSON.parse(e.target.value) as ApiPrdTask); } catch { /* noop */ }
          }}
          className="h-[60vh] w-full rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] p-3 font-mono text-[11px] text-[var(--text-primary)] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50"
          spellCheck={false}
        />
      ) : (
        <>
          <Field label="Title">
            <input type="text" value={task.title ?? ''} onChange={(e) => handleChange('title', e.target.value)}
              className="w-full rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-3 h-9 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50" />
          </Field>

          <div className="grid grid-cols-2 gap-3">
            <Field label="Priority">
              <select value={task.priority ?? ''} onChange={(e) => handleChange('priority', e.target.value)}
                className="w-full rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-3 h-9 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50">
                <option value="1">P1</option>
                <option value="2">P2</option>
                <option value="3">P3</option>
              </select>
            </Field>
            <Field label="Category">
              <input type="text" value={task.category ?? ''} onChange={(e) => handleChange('category', e.target.value)}
                className="w-full rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-3 h-9 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50" />
            </Field>
          </div>

          <Field label="Description">
            <textarea value={task.description ?? ''} onChange={(e) => handleChange('description', e.target.value)}
              className="w-full rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] p-3 text-sm text-[var(--text-primary)] min-h-[80px] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50" />
          </Field>

          <Field label="Objective">
            <input type="text" value={task.objective ?? ''} onChange={(e) => handleChange('objective', e.target.value)}
              className="w-full rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-3 h-9 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50" />
          </Field>

          <Field label="Dependencies (comma-separated IDs)">
            <textarea value={(task.dependencies ?? []).join(', ')}
              onChange={(e) => handleChange('dependencies', e.target.value.split(',').map((s) => s.trim()).filter(Boolean))}
              className="w-full rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] p-3 text-sm text-[var(--text-primary)] min-h-[48px] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50" />
          </Field>

          <Field label="Exclusive resources (comma-separated)">
            <textarea value={(task.exclusiveResources ?? []).join(', ')}
              onChange={(e) => handleChange('exclusiveResources', e.target.value.split(',').map((s) => s.trim()).filter(Boolean))}
              className="w-full rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] p-3 text-sm text-[var(--text-primary)] min-h-[48px] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50" />
          </Field>

          <Field label="Notes">
            <textarea value={task.notes ?? ''} onChange={(e) => handleChange('notes', e.target.value)}
              className="w-full rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] p-3 text-sm text-[var(--text-primary)] min-h-[60px] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50" />
          </Field>
        </>
      )}
    </div>
  );
};

// ── Generate panel ────────────────────────────────────────────────────────────

const GeneratePanel: React.FC<{ onDone: () => void }> = ({ onDone }) => {
  const [fromPath, setFromPath] = useState('');
  const [tool, setTool] = useState('');
  const [instructions, setInstructions] = useState('');
  const [dryRun, setDryRun] = useState(false);
  const [promote, setPromote] = useState(false);
  const [isRunning, setIsRunning] = useState(false);
  const [result, setResult] = useState<ApiPrdGenerateResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    if (!fromPath.trim()) { setError('Brief file path is required.'); return; }
    setIsRunning(true);
    setError(null);
    setResult(null);
    try {
      const res = await prdGenerate({
        fromPath: fromPath.trim(),
        tool: tool.trim() || undefined,
        instructions: instructions.trim() || undefined,
        dryRun,
        promote,
      });
      setResult(res);
      if (promote && !dryRun) onDone();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Generation failed.');
    } finally {
      setIsRunning(false);
    }
  };

  return (
    <div className="space-y-4">
      <p className="text-sm text-[var(--text-secondary)]">
        Generate a new PRD from a brief file using the <span className="font-mono text-[11px]">macc-prd-planner</span> skill.
      </p>

      <Field label="Brief file path">
        <input type="text" value={fromPath} onChange={(e) => setFromPath(e.target.value)}
          placeholder="docs/brief.md"
          className="field-input" />
      </Field>

      <Field label="Tool (optional)">
        <input type="text" value={tool} onChange={(e) => setTool(e.target.value)}
          placeholder="claude"
          className="field-input" />
      </Field>

      <Field label="Instructions (optional)">
        <textarea value={instructions} onChange={(e) => setInstructions(e.target.value)}
          placeholder="Additional instructions appended to the prompt"
          className="field-textarea" />
      </Field>

      <div className="flex gap-4">
        <label className="flex cursor-pointer items-center gap-2 text-sm text-[var(--text-secondary)]">
          <input type="checkbox" checked={dryRun} onChange={(e) => setDryRun(e.target.checked)} className="h-4 w-4 accent-[var(--accent)]" />
          Dry run (preview prompt only)
        </label>
        <label className="flex cursor-pointer items-center gap-2 text-sm text-[var(--text-secondary)]">
          <input type="checkbox" checked={promote} onChange={(e) => setPromote(e.target.checked)} className="h-4 w-4 accent-[var(--accent)]" />
          Auto-promote after generation
        </label>
      </div>

      {error && <ErrorBox message={error} />}

      {result && (
        <div className="rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] p-3 text-sm">
          <p className="font-medium text-[var(--text-primary)]">Status: {result.status}</p>
          {result.runId && <p className="text-[var(--text-muted)] text-xs">Run: {result.runId}</p>}
          {result.targetDir && <p className="font-mono text-[11px] text-[var(--text-muted)]">{result.targetDir}</p>}
          {result.prompt && (
            <details className="mt-2">
              <summary className="cursor-pointer text-xs text-[var(--accent)]">View generated prompt</summary>
              <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap font-mono text-[10px] text-[var(--text-secondary)]">{result.prompt}</pre>
            </details>
          )}
        </div>
      )}

      <Button
        className="w-full border-transparent bg-[var(--accent)] text-white hover:brightness-110 h-9"
        onClick={() => void run()}
        disabled={isRunning}
      >
        {isRunning ? 'Generating…' : 'Generate PRD'}
      </Button>
    </div>
  );
};

// ── Audit panel ───────────────────────────────────────────────────────────────

const AuditPanel: React.FC<{ prdFilePath: string | null }> = ({ prdFilePath }) => {
  const [prdPath, setPrdPath] = useState(prdFilePath ?? 'prd.json');
  const [tool, setTool] = useState('');
  const [instructions, setInstructions] = useState('');
  const [referenceBranch, setReferenceBranch] = useState('');
  const [diffStat, setDiffStat] = useState(false);
  const [dryRun, setDryRun] = useState(false);
  const [isRunning, setIsRunning] = useState(false);
  const [result, setResult] = useState<ApiPrdAuditResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setIsRunning(true);
    setError(null);
    setResult(null);
    try {
      const res = await prdAudit({
        prdPath: prdPath.trim() || undefined,
        tool: tool.trim() || undefined,
        instructions: instructions.trim() || undefined,
        referenceBranch: referenceBranch.trim() || undefined,
        diffStat,
        dryRun,
      });
      setResult(res);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Audit failed.');
    } finally {
      setIsRunning(false);
    }
  };

  return (
    <div className="space-y-4">
      <p className="text-sm text-[var(--text-secondary)]">
        Enrich the PRD from commit history and delivered code.
      </p>

      <Field label="PRD file path">
        <input type="text" value={prdPath} onChange={(e) => setPrdPath(e.target.value)}
          className="field-input" />
      </Field>

      <Field label="Tool (optional — omit to preview prompt only)">
        <input type="text" value={tool} onChange={(e) => setTool(e.target.value)}
          placeholder="claude"
          className="field-input" />
      </Field>

      <Field label="Reference branch (optional)">
        <input type="text" value={referenceBranch} onChange={(e) => setReferenceBranch(e.target.value)}
          placeholder="main"
          className="field-input" />
      </Field>

      <Field label="Instructions (optional)">
        <textarea value={instructions} onChange={(e) => setInstructions(e.target.value)}
          placeholder="Additional context for the audit"
          className="field-textarea" />
      </Field>

      <div className="flex gap-4">
        <label className="flex cursor-pointer items-center gap-2 text-sm text-[var(--text-secondary)]">
          <input type="checkbox" checked={diffStat} onChange={(e) => setDiffStat(e.target.checked)} className="h-4 w-4 accent-[var(--accent)]" />
          Include diff --stat
        </label>
        <label className="flex cursor-pointer items-center gap-2 text-sm text-[var(--text-secondary)]">
          <input type="checkbox" checked={dryRun} onChange={(e) => setDryRun(e.target.checked)} className="h-4 w-4 accent-[var(--accent)]" />
          Dry run
        </label>
      </div>

      {error && <ErrorBox message={error} />}

      {result && (
        <div className="rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] p-3 text-sm space-y-1">
          <p className="text-[var(--text-primary)]">
            Completed: <strong>{result.completedWithContext}</strong> · Todo: <strong>{result.todoTasks}</strong>
          </p>
          <p className="text-xs text-[var(--text-muted)]">
            {result.dispatched ? 'Dispatched to tool.' : 'Prompt only (no tool invoked).'}
          </p>
          {result.prompt && (
            <details className="mt-2">
              <summary className="cursor-pointer text-xs text-[var(--accent)]">View audit prompt</summary>
              <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap font-mono text-[10px] text-[var(--text-secondary)]">{result.prompt}</pre>
            </details>
          )}
        </div>
      )}

      <Button
        className="w-full border-transparent bg-[var(--accent)] text-white hover:brightness-110 h-9"
        onClick={() => void run()}
        disabled={isRunning}
      >
        {isRunning ? 'Running audit…' : 'Run audit'}
      </Button>
    </div>
  );
};

// ── Promote panel ─────────────────────────────────────────────────────────────

const PromotePanel: React.FC<{ onDone: () => void }> = ({ onDone }) => {
  const [sourcePath, setSourcePath] = useState('.macc/generated/prd/macc-prd-planner/');
  const [destPath, setDestPath] = useState('prd.json');
  const [isRunning, setIsRunning] = useState(false);
  const [result, setResult] = useState<ApiPrdPromoteResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    if (!sourcePath.trim()) { setError('Source path is required.'); return; }
    setIsRunning(true);
    setError(null);
    setResult(null);
    try {
      const res = await prdPromote({
        sourcePath: sourcePath.trim(),
        destPath: destPath.trim() || undefined,
        yes: true,
      });
      setResult(res);
      if (res.promoted) onDone();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Promote failed.');
    } finally {
      setIsRunning(false);
    }
  };

  return (
    <div className="space-y-4">
      <p className="text-sm text-[var(--text-secondary)]">
        Promote a generated PRD to the active <span className="font-mono text-[11px]">prd.json</span>.
      </p>

      <Field label="Source path (generated PRD)">
        <input type="text" value={sourcePath} onChange={(e) => setSourcePath(e.target.value)}
          className="field-input" />
      </Field>

      <Field label="Destination path">
        <input type="text" value={destPath} onChange={(e) => setDestPath(e.target.value)}
          className="field-input" />
      </Field>

      {error && <ErrorBox message={error} />}

      {result && (
        <div className="rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] p-3 text-sm space-y-1">
          <p className="font-medium" style={{ color: result.promoted ? 'var(--success)' : 'var(--warning)' }}>
            {result.promoted ? 'Promoted successfully.' : 'Not promoted.'}
          </p>
          <p className="font-mono text-[11px] text-[var(--text-muted)]">{result.sourcePath} → {result.destPath}</p>
          {result.backedUpExisting && <p className="text-xs text-[var(--text-muted)]">Existing PRD backed up.</p>}
        </div>
      )}

      <Button
        className="w-full border-transparent bg-[var(--accent)] text-white hover:brightness-110 h-9"
        onClick={() => void run()}
        disabled={isRunning}
      >
        {isRunning ? 'Promoting…' : 'Promote to prd.json'}
      </Button>
    </div>
  );
};

// ── Runs panel ────────────────────────────────────────────────────────────────

const RunsPanel: React.FC<{ onPromote: () => void }> = ({ onPromote }) => {
  const [runs, setRuns] = useState<ApiPrdRunEntry[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        const data = await prdListRuns();
        setRuns(data);
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to load runs.');
      } finally {
        setIsLoading(false);
      }
    })();
  }, []);

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <p className="text-sm text-[var(--text-secondary)]">
          PRD generation runs stored in <span className="font-mono text-[11px]">.macc/generated/prd/</span>
        </p>
        <Button className="h-7 border-[var(--border)] bg-[var(--bg-secondary)] px-2 text-xs" onClick={onPromote}>
          Promote run →
        </Button>
      </div>

      {error && <ErrorBox message={error} />}

      {isLoading ? (
        <p className="text-sm text-[var(--text-muted)]">Loading…</p>
      ) : runs.length === 0 ? (
        <p className="text-sm text-[var(--text-muted)]">No generation runs found.</p>
      ) : (
        <ul className="divide-y divide-[var(--border-subtle)] overflow-hidden rounded-md border border-[var(--border)]">
          {runs.map((run) => (
            <li key={run.runId} className="px-3 py-2.5">
              <p className="font-mono text-[11px] font-medium text-[var(--text-primary)]">{run.runId}</p>
              <p className="mt-0.5 font-mono text-[10px] text-[var(--text-muted)] truncate">{run.path}</p>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
};

// ── Small shared components ───────────────────────────────────────────────────

const Field: React.FC<{ label: string; children: React.ReactNode }> = ({ label, children }) => (
  <div className="space-y-1.5">
    <label className="block text-xs font-medium text-[var(--text-secondary)]">{label}</label>
    {children}
  </div>
);

const ErrorBox: React.FC<{ message: string }> = ({ message }) => (
  <div className="rounded-md border border-[var(--error)]/40 bg-[var(--error)]/10 px-3 py-2 text-xs text-[var(--text-primary)]">
    {message}
  </div>
);

export default PrdPage;
