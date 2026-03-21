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
import { 
  getRegistryTasks, 
  requeueTask, 
  reassignTask, 
  abandonTask,
  getConfig 
} from '../../api/client';
import type { ApiRegistryTask } from '../../api/models';
import { Button } from '../../components/Button';
import { StatusBadge, type StatusTone } from '../../components/StatusBadge';
import { RightDrawer } from '../../components/RightDrawer';
import { ConfirmDialog } from '../../components/ConfirmDialog';
import * as Icons from '../../components/icons';
import { cn } from '../../components/styles';

const columnHelper = createColumnHelper<ApiRegistryTask>();

const Registry: React.FC = () => {
  const [tasks, setTasks] = useState<ApiRegistryTask[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [globalFilter, setGlobalFilter] = useState('');
  const [sorting, setSorting] = useState<SortingState>([]);
  const [availableTools, setAvailableTools] = useState<string[]>([]);
  
  // Filter state
  const [stateFilter, setStateFilter] = useState<string>('');
  const [toolFilter, setToolFilter] = useState<string>('');
  const [priorityFilter, setPriorityFilter] = useState<string>('');

  // Operator Actions state
  const [showReassignDialog, setShowReassignDialog] = useState(false);
  const [showAbandonDialog, setShowAbandonDialog] = useState(false);
  const [reassignTool, setReassignTool] = useState('');
  const [reassignJustification, setReassignJustification] = useState('');

  const tableContainerRef = useRef<HTMLDivElement>(null);

  const fetchRegistry = useCallback(async (silent = false) => {
    if (!silent) setIsLoading(true);
    try {
      const data = await getRegistryTasks();
      setTasks(data || []);
    } catch (err) {
      console.error('Failed to fetch registry tasks:', err);
    } finally {
      if (!silent) setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchRegistry();
    
    // Auto-refresh every 10s
    const interval = setInterval(() => {
      fetchRegistry(true);
    }, 10000);

    // Fetch config to get available tools
    getConfig().then(config => {
      setAvailableTools(config.enabledTools || []);
    }).catch(console.error);

    return () => clearInterval(interval);
  }, [fetchRegistry]);

  const filteredTasks = useMemo(() => {
    return tasks.filter(task => {
      if (stateFilter && task.state !== stateFilter) return false;
      if (toolFilter && task.tool !== toolFilter) return false;
      if (priorityFilter && task.priority !== priorityFilter) return false;
      return true;
    });
  }, [tasks, stateFilter, toolFilter, priorityFilter]);

  const selectedTask = useMemo(() => 
    tasks.find(t => t.id === selectedTaskId) || null
  , [tasks, selectedTaskId]);

  const columns = useMemo(() => [
    columnHelper.accessor('id', {
      header: 'ID',
      cell: info => <span className="font-mono text-xs text-[var(--text-secondary)]">{info.getValue()}</span>,
      size: 140,
    }),
    columnHelper.accessor('title', {
      header: 'Title',
      cell: info => <span className="font-medium truncate block max-w-[300px]">{info.getValue() || '(No Title)'}</span>,
      size: 250,
    }),
    columnHelper.accessor('state', {
      header: 'State',
      cell: info => {
        const state = info.getValue() as string;
        let tone: StatusTone = 'todo';
        if (state === 'active') tone = 'active';
        if (state === 'blocked') tone = 'blocked';
        if (state === 'failed' || state === 'abandoned') tone = 'failed';
        if (state === 'merged') tone = 'merged';
        return <StatusBadge status={state} tone={tone} />;
      },
      size: 120,
    }),
    columnHelper.accessor('tool', {
      header: 'Tool',
      cell: info => <span className="text-xs font-mono uppercase opacity-70">{info.getValue() || '-'}</span>,
      size: 100,
    }),
    columnHelper.accessor('priority', {
      header: 'Prio',
      cell: info => info.getValue() || '-',
      size: 60,
    }),
    columnHelper.accessor('attempts', {
      header: 'Try',
      cell: info => info.getValue() || 0,
      size: 60,
    }),
    columnHelper.accessor('heartbeat', {
      header: 'Heartbeat',
      cell: info => info.getValue() ? new Date(info.getValue()!).toLocaleTimeString() : '-',
      size: 120,
    }),
    columnHelper.accessor('delayedUntil', {
      header: 'Delay',
      cell: info => info.getValue() ? new Date(info.getValue()!).toLocaleTimeString() : '-',
      size: 120,
    }),
    columnHelper.accessor('lastErrorCode', {
      header: 'Err',
      cell: info => info.getValue() ? <span className="text-rose-500 font-mono text-xs">{info.getValue()}</span> : '-',
      size: 80,
    }),
  ], []);

  const table = useReactTable({
    data: filteredTasks,
    columns,
    state: {
      sorting,
      globalFilter,
    },
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
    estimateSize: () => 48,
    overscan: 10,
  });

  const handleRequeue = async () => {
    if (!selectedTaskId) return;
    try {
      await requeueTask(selectedTaskId, { kind: 'requeue' });
      fetchRegistry(true);
    } catch (err) {
      console.error('Failed to requeue task:', err);
    }
  };

  const handleAbandon = async () => {
    if (!selectedTaskId) return;
    try {
      await abandonTask(selectedTaskId, { kind: 'abandon' });
      setShowAbandonDialog(false);
      fetchRegistry(true);
    } catch (err) {
      console.error('Failed to abandon task:', err);
    }
  };

  const handleReassign = async () => {
    if (!selectedTaskId || !reassignTool) return;
    try {
      await reassignTask(selectedTaskId, { 
        kind: 'reassign', 
        tool: reassignTool, 
        justification: reassignJustification 
      });
      setShowReassignDialog(false);
      setReassignJustification('');
      fetchRegistry(true);
    } catch (err) {
      console.error('Failed to reassign task:', err);
    }
  };

  const handleExport = () => {
    const blob = new Blob([JSON.stringify(tasks, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `macc-registry-${new Date().toISOString()}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="flex h-[calc(100vh-80px)] flex-col gap-4 overflow-hidden p-4">
      <header className="flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-2xl font-bold text-[var(--text-primary)]">Task Registry</h1>
            <p className="text-sm text-[var(--text-muted)]">Real-time task state and operator controls</p>
          </div>
          
          <div className="flex items-center gap-3">
            <Button onClick={() => fetchRegistry()} className="h-10 w-10 p-0">
              <Icons.RefreshIcon className={cn("h-4 w-4", isLoading && "animate-spin")} />
            </Button>

            <Button onClick={handleExport} className="gap-2">
              <Icons.DownloadIcon className="h-4 w-4" />
              Export
            </Button>
          </div>
        </div>

        <div className="flex items-center gap-4 bg-[var(--bg-secondary)] p-3 rounded-xl border border-[var(--border)]">
          <div className="relative flex-1">
            <Icons.SearchIcon className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--text-muted)]" />
            <input
              type="text"
              value={globalFilter ?? ''}
              onChange={e => setGlobalFilter(e.target.value)}
              placeholder="Search ID or Title..."
              className="h-10 w-full rounded-lg border border-[var(--border)] bg-[var(--bg-primary)] pl-10 pr-4 text-sm outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
            />
          </div>

          <select
            value={stateFilter}
            onChange={e => setStateFilter(e.target.value)}
            className="h-10 rounded-lg border border-[var(--border)] bg-[var(--bg-primary)] px-3 text-sm outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
          >
            <option value="">All States</option>
            <option value="todo">Todo</option>
            <option value="active">Active</option>
            <option value="blocked">Blocked</option>
            <option value="merged">Merged</option>
            <option value="failed">Failed</option>
            <option value="abandoned">Abandoned</option>
          </select>

          <select
            value={toolFilter}
            onChange={e => setToolFilter(e.target.value)}
            className="h-10 rounded-lg border border-[var(--border)] bg-[var(--bg-primary)] px-3 text-sm outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
          >
            <option value="">All Tools</option>
            {availableTools.map(tool => (
              <option key={tool} value={tool}>{tool.toUpperCase()}</option>
            ))}
          </select>

          <select
            value={priorityFilter}
            onChange={e => setPriorityFilter(e.target.value)}
            className="h-10 rounded-lg border border-[var(--border)] bg-[var(--bg-primary)] px-3 text-sm outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
          >
            <option value="">All Priorities</option>
            <option value="1">P1 - High</option>
            <option value="2">P2 - Medium</option>
            <option value="3">P3 - Low</option>
          </select>
        </div>
      </header>

      <div 
        ref={tableContainerRef}
        className="flex-1 overflow-auto rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] shadow-sm"
      >
        <table className="w-full text-left text-sm border-collapse">
          <thead className="sticky top-0 z-10 bg-[var(--bg-secondary)] border-b border-[var(--border)]">
            {table.getHeaderGroups().map(headerGroup => (
              <tr key={headerGroup.id}>
                {headerGroup.headers.map(header => (
                  <th 
                    key={header.id}
                    onClick={header.column.getToggleSortingHandler()}
                    className="px-4 py-3 font-semibold text-[var(--text-secondary)] cursor-pointer hover:bg-[var(--bg-hover)] transition-colors"
                    style={{ width: header.getSize() }}
                  >
                    <div className="flex items-center gap-2">
                      {flexRender(header.column.columnDef.header, header.getContext())}
                      {{
                        asc: <Icons.ChevronUpIcon className="h-3 w-3" />,
                        desc: <Icons.ChevronDownIcon className="h-3 w-3" />,
                      }[header.column.getIsSorted() as string] ?? null}
                    </div>
                  </th>
                ))}
              </tr>
            ))}
          </thead>
          <tbody style={{ height: `${rowVirtualizer.getTotalSize()}px`, position: 'relative' }}>
            {rowVirtualizer.getVirtualItems().map(virtualRow => {
              const row = rows[virtualRow.index];
              return (
                <tr 
                  key={row.id}
                  data-index={virtualRow.index}
                  ref={rowVirtualizer.measureElement}
                  onClick={() => setSelectedTaskId(row.original.id)}
                  className={cn(
                    "absolute w-full hover:bg-[var(--bg-secondary)]/50 cursor-pointer transition-colors border-b border-[var(--border)]/50",
                    selectedTaskId === row.original.id && "bg-[var(--accent)]/10 ring-1 ring-inset ring-[var(--accent)]/50"
                  )}
                  style={{ transform: `translateY(${virtualRow.start}px)` }}
                >
                  {row.getVisibleCells().map(cell => (
                    <td key={cell.id} className="px-4 py-3 align-middle" style={{ width: cell.column.getSize() }}>
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </td>
                  ))}
                </tr>
              );
            })}
          </tbody>
        </table>
        {rows.length === 0 && !isLoading && (
          <div className="flex flex-col items-center justify-center p-20 text-[var(--text-muted)]">
            <Icons.FolderOpenIcon className="h-10 w-10 opacity-20 mb-4" />
            <p>No tasks found in registry.</p>
          </div>
        )}
      </div>

      {/* Task Inspector */}
      <RightDrawer
        open={!!selectedTaskId}
        onOpenChange={(open) => !open && setSelectedTaskId(null)}
        title={selectedTask?.id || 'Task Details'}
        description={selectedTask?.title || undefined}
        widthClassName="w-full max-w-2xl"
        footer={
          <div className="flex items-center gap-3">
            <Button onClick={handleRequeue} className="flex-1 gap-2">
              <Icons.RefreshIcon className="h-4 w-4" />
              Requeue
            </Button>
            <Button onClick={() => {
              setReassignTool(selectedTask?.tool || '');
              setShowReassignDialog(true);
            }} className="flex-1 gap-2">
              <Icons.SwitchIcon className="h-4 w-4" />
              Reassign
            </Button>
            <Button onClick={() => setShowAbandonDialog(true)} className="flex-1 gap-2 text-rose-500 hover:text-rose-600 border-rose-500/20">
              <Icons.XCircleIcon className="h-4 w-4" />
              Abandon
            </Button>
          </div>
        }
      >
        {selectedTask && (
          <div className="space-y-8">
            <div className="grid grid-cols-2 gap-4">
              <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-primary)]/50 p-3">
                <label className="text-[10px] font-bold uppercase tracking-wider text-[var(--text-muted)] block mb-1">Current Phase</label>
                <div className="text-sm font-mono">{selectedTask.currentPhase || 'N/A'}</div>
              </div>
              <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-primary)]/50 p-3">
                <label className="text-[10px] font-bold uppercase tracking-wider text-[var(--text-muted)] block mb-1">Updated At</label>
                <div className="text-sm font-mono">{selectedTask.updatedAt ? new Date(selectedTask.updatedAt).toLocaleString() : 'N/A'}</div>
              </div>
            </div>

            {selectedTask.description && (
              <div className="space-y-1.5">
                <label className="text-[10px] font-bold uppercase tracking-wider text-[var(--text-muted)] px-1">Description</label>
                <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-primary)]/50 p-4 text-sm leading-relaxed">
                  {selectedTask.description}
                </div>
              </div>
            )}

            {(selectedTask.objective || selectedTask.result) && (
              <div className="grid grid-cols-2 gap-4">
                {selectedTask.objective && (
                  <div className="space-y-1.5">
                    <label className="text-[10px] font-bold uppercase tracking-wider text-[var(--text-muted)] px-1">Objective</label>
                    <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-primary)]/50 p-3 text-xs leading-relaxed">
                      {selectedTask.objective}
                    </div>
                  </div>
                )}
                {selectedTask.result && (
                  <div className="space-y-1.5">
                    <label className="text-[10px] font-bold uppercase tracking-wider text-[var(--text-muted)] px-1">Expected Result</label>
                    <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-primary)]/50 p-3 text-xs leading-relaxed italic">
                      {selectedTask.result}
                    </div>
                  </div>
                )}
              </div>
            )}

            {selectedTask.steps && selectedTask.steps.length > 0 && (
              <div className="space-y-3">
                <label className="text-[10px] font-bold uppercase tracking-wider text-[var(--text-muted)] px-1">Steps</label>
                <div className="space-y-2">
                  {selectedTask.steps.map((step, idx) => (
                    <div key={idx} className="flex gap-3 text-sm">
                      <span className="text-[var(--text-muted)] font-mono tabular-nums">{idx + 1}.</span>
                      <span className="text-[var(--text-secondary)]">{step}</span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {selectedTask.lastError && (
              <div className="rounded-xl border border-rose-500/20 bg-rose-500/5 p-4 space-y-2">
                <div className="flex items-center gap-2 text-rose-500 text-xs font-bold uppercase">
                  <Icons.AlertTriangleIcon className="h-4 w-4" />
                  Error: {selectedTask.lastErrorCode || 'FAILED'}
                </div>
                <div className="text-sm text-rose-200/80 font-mono break-words bg-black/20 p-2 rounded-lg leading-relaxed">
                  {selectedTask.lastError}
                </div>
              </div>
            )}

            <div className="space-y-4">
              <h3 className="text-sm font-bold uppercase tracking-wider text-[var(--text-secondary)] px-1">Event History</h3>
              <div className="relative space-y-4 before:absolute before:inset-0 before:ml-[11px] before:h-full before:w-0.5 before:bg-[var(--border)]">
                {(selectedTask.events || []).length > 0 ? (
                  selectedTask.events.map((event, idx) => (
                    <div key={idx} className="relative flex items-start gap-4 pl-8">
                      <div className={cn(
                        "absolute left-0 mt-1 h-6 w-6 rounded-full border-2 border-[var(--bg-secondary)] bg-[var(--bg-card)] flex items-center justify-center",
                        event.status === 'success' ? "text-emerald-500" : event.status === 'failed' ? "text-rose-500" : "text-[var(--accent)]"
                      )}>
                        {event.status === 'success' ? <Icons.CheckIcon className="h-3 w-3" /> : 
                         event.status === 'failed' ? <Icons.XIcon className="h-3 w-3" /> : 
                         <Icons.ActivityIcon className="h-3 w-3" />}
                      </div>
                      <div className="flex-1 space-y-1">
                        <div className="flex items-center justify-between gap-4">
                          <span className="text-sm font-semibold">{event.eventType}</span>
                          <span className="text-[10px] font-mono text-[var(--text-muted)]">{event.ts ? new Date(event.ts).toLocaleTimeString() : ''}</span>
                        </div>
                        <p className="text-xs text-[var(--text-secondary)] leading-relaxed">
                          {event.message}
                        </p>
                      </div>
                    </div>
                  ))
                ) : (
                  <div className="text-xs text-[var(--text-muted)] pl-8 italic">No events recorded.</div>
                )}
              </div>
            </div>
          </div>
        )}
      </RightDrawer>

      <ConfirmDialog
        open={showAbandonDialog}
        onOpenChange={setShowAbandonDialog}
        title="Mark Task as Abandoned"
        description="This will stop any active work and move the task to a terminal 'failed' state. It will not be retried automatically."
        confirmLabel="Abandon Task"
        intent="danger"
        onConfirm={handleAbandon}
      />

      <ConfirmDialog
        open={showReassignDialog}
        onOpenChange={setShowReassignDialog}
        title="Reassign Task Tool"
        description="Changing the assigned tool will requeue the task. Please provide a reason for the reassignment."
        confirmLabel="Reassign"
        onConfirm={handleReassign}
      >
        <div className="mt-4 space-y-4">
          <div className="space-y-1.5">
            <label className="text-xs font-bold uppercase text-[var(--text-secondary)]">New Tool</label>
            <select
              value={reassignTool}
              onChange={e => setReassignTool(e.target.value)}
              className="w-full bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg px-3 h-10 text-sm outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
            >
              <option value="">Select a tool...</option>
              {availableTools.map(tool => (
                <option key={tool} value={tool}>{tool.toUpperCase()}</option>
              ))}
            </select>
          </div>
          <div className="space-y-1.5">
            <label className="text-xs font-bold uppercase text-[var(--text-secondary)]">Justification</label>
            <textarea
              value={reassignJustification}
              onChange={e => setReassignJustification(e.target.value)}
              placeholder="Why are you reassigning this task?"
              className="w-full bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg p-3 min-h-[80px] text-sm outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
            />
          </div>
        </div>
      </ConfirmDialog>
    </div>
  );
};

export default Registry;
