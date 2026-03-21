import React, { useState, useEffect, useMemo, useRef } from 'react';
import { 
  useReactTable, 
  getCoreRowModel, 
  getSortedRowModel, 
  getFilteredRowModel,
  flexRender,
  createColumnHelper,
  type SortingState
} from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { getPrd, updatePrd } from '../api/client';
import type { ApiPrdTask, JsonValue } from '../api/models';
import { Button } from '../components/Button';
import * as Icons from '../components/icons';
import { cn } from '../components/styles';

const columnHelper = createColumnHelper<ApiPrdTask>();

const PrdPage: React.FC = () => {
  const [tasks, setTasks] = useState<ApiPrdTask[]>([]);
  const [metadata, setMetadata] = useState<Record<string, JsonValue>>({});
  const [isLoading, setIsLoading] = useState(true);
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [globalFilter, setGlobalFilter] = useState('');
  const [sorting, setSorting] = useState<SortingState>([]);
  const [isJsonMode, setIsJsonMode] = useState(false);
  const [unsavedChanges, setUnsavedChanges] = useState(false);
  const [validationErrors, setValidationErrors] = useState<string[]>([]);

  const tableContainerRef = useRef<HTMLDivElement>(null);

  const fetchPrdData = async () => {
    setIsLoading(true);
    try {
      const data = await getPrd();
      setTasks(data.tasks || []);
      setMetadata(data.metadata || {});
    } catch (err) {
      console.error('Failed to fetch PRD data:', err);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    fetchPrdData();
  }, []);

  const selectedTask = useMemo(() => 
    tasks.find(t => t.id === selectedTaskId) || null
  , [tasks, selectedTaskId]);

  const columns = useMemo(() => [
    columnHelper.accessor('id', {
      header: 'ID',
      cell: info => <span className="font-mono text-xs">{info.getValue()}</span>,
      size: 150,
    }),
    columnHelper.accessor('title', {
      header: 'Title',
      cell: info => <span className="font-medium">{info.getValue() || '(No Title)'}</span>,
      size: 300,
    }),
    columnHelper.accessor('category', {
      header: 'Category',
      size: 120,
    }),
    columnHelper.accessor('priority', {
      header: 'Prio',
      size: 80,
    }),
    columnHelper.accessor('dependencies', {
      header: 'Deps',
      cell: info => (info.getValue() || []).length,
      size: 80,
    }),
    columnHelper.accessor('exclusiveResources', {
      header: 'Resources',
      cell: info => (info.getValue() || []).length,
      size: 100,
    }),
  ], []);

  const table = useReactTable({
    data: tasks,
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
    estimateSize: () => 45,
    overscan: 10,
  });

  const handleTaskUpdate = (updatedTask: ApiPrdTask) => {
    setTasks(prev => prev.map(t => t.id === updatedTask.id ? updatedTask : t));
    setUnsavedChanges(true);
  };

  const validatePrd = () => {
    const errors: string[] = [];
    const taskIds = new Set(tasks.map(t => t.id));

    tasks.forEach(task => {
      if (!task.id) errors.push(`Task with missing ID found.`);
      if (!task.title) errors.push(`Task ${task.id} is missing a title.`);
      
      (task.dependencies || []).forEach(depId => {
        if (!taskIds.has(depId)) {
          errors.push(`Task ${task.id} has invalid dependency: ${depId}`);
        }
      });

      // Simple circular dependency check (just direct for now)
      if ((task.dependencies || []).includes(task.id)) {
        errors.push(`Task ${task.id} cannot depend on itself.`);
      }
    });

    setValidationErrors(errors);
    return errors.length === 0;
  };

  const handleSave = async () => {
    if (!validatePrd()) return;

    setIsSaving(true);
    try {
      await updatePrd({ tasks, metadata });
      setUnsavedChanges(false);
      setValidationErrors([]);
      alert('PRD saved successfully');
    } catch (err) {
      console.error('Failed to save PRD:', err);
      alert('Failed to save PRD');
    } finally {
      setIsSaving(false);
    }
  };

  const handleDiscard = () => {
    if (confirm('Are you sure you want to discard all unsaved changes?')) {
      fetchPrdData();
      setUnsavedChanges(false);
      setValidationErrors([]);
    }
  };

  return (
    <div className="flex h-[calc(100vh-80px)] flex-col gap-4 overflow-hidden p-4">
      <header className="flex items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold text-[var(--text-primary)]">PRD Editor</h1>
          <p className="text-sm text-[var(--text-muted)]">Manage tasks and requirements</p>
        </div>
        
        <div className="flex items-center gap-3">
          <div className="relative">
            <Icons.SearchIcon className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--text-muted)]" />
            <input
              type="text"
              value={globalFilter ?? ''}
              onChange={e => setGlobalFilter(e.target.value)}
              placeholder="Search tasks..."
              className="h-10 w-64 rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] pl-10 pr-4 text-sm focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
            />
          </div>
          
          {unsavedChanges && (
            <div className="flex items-center gap-2">
              <Button onClick={handleDiscard} className="bg-transparent border-[var(--border)]" disabled={isSaving}>Discard</Button>
              <Button onClick={handleSave} disabled={isSaving}>
                {isSaving ? 'Saving...' : 'Save Changes'}
              </Button>
            </div>
          )}
        </div>
      </header>

      {validationErrors.length > 0 && (
        <div className="rounded-xl border border-rose-500/50 bg-rose-500/10 p-4">
          <h3 className="text-sm font-bold text-rose-500 flex items-center gap-2">
            <Icons.AlertTriangleIcon className="h-4 w-4" />
            Validation Errors
          </h3>
          <ul className="mt-2 list-inside list-disc text-xs text-rose-400 space-y-1">
            {validationErrors.slice(0, 5).map((err, i) => (
              <li key={i}>{err}</li>
            ))}
            {validationErrors.length > 5 && <li>...and {validationErrors.length - 5} more</li>}
          </ul>
        </div>
      )}

      <div className="flex flex-1 gap-4 overflow-hidden">
        {/* Table Section */}
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
              <p>No tasks found matching your criteria.</p>
            </div>
          )}
        </div>

        {/* Detail Pane */}
        {selectedTask && (
          <aside className="w-[450px] flex flex-col rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] shadow-lg overflow-hidden animate-in slide-in-from-right duration-200">
            <header className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--bg-secondary)] p-4">
              <div className="flex items-center gap-2 overflow-hidden">
                <Button className="p-0 h-8 w-8 bg-transparent border-none hover:bg-[var(--bg-hover)] shadow-none" onClick={() => setSelectedTaskId(null)}>
                  <Icons.XIcon className="h-4 w-4" />
                </Button>
                <h2 className="font-bold truncate text-[var(--text-primary)]">{selectedTask.id}</h2>
              </div>
              <div className="flex items-center gap-2">
                <button 
                  onClick={() => setIsJsonMode(!isJsonMode)}
                  className={cn(
                    "px-2 py-1 rounded-md text-[10px] font-bold uppercase tracking-wider transition-colors",
                    isJsonMode ? "bg-[var(--accent)] text-white" : "bg-[var(--bg-secondary)] text-[var(--text-muted)] border border-[var(--border)]"
                  )}
                >
                  JSON
                </button>
              </div>
            </header>

            <div className="flex-1 overflow-y-auto p-4 space-y-6">
              {isJsonMode ? (
                <textarea
                  value={JSON.stringify(selectedTask, null, 2)}
                  onChange={e => {
                    try {
                      const parsed = JSON.parse(e.target.value);
                      handleTaskUpdate(parsed);
                    } catch {
                      // Ignore invalid JSON during typing
                    }
                  }}
                  className="w-full h-full font-mono text-xs bg-[var(--bg-secondary)] text-[var(--text-primary)] p-4 rounded-xl border border-[var(--border)] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]"
                  spellCheck={false}
                />
              ) : (
                <TaskForm task={selectedTask} onChange={handleTaskUpdate} />
              )}
            </div>
          </aside>
        )}
      </div>
    </div>
  );
};

interface TaskFormProps {
  task: ApiPrdTask;
  onChange: (task: ApiPrdTask) => void;
}

const TaskForm: React.FC<TaskFormProps> = ({ task, onChange }) => {
  const handleChange = (field: keyof ApiPrdTask, value: JsonValue) => {
    onChange({ ...task, [field]: value } as ApiPrdTask);
  };

  return (
    <div className="space-y-4">
      <div className="space-y-1.5">
        <label className="text-[10px] font-bold uppercase text-[var(--text-secondary)] px-1">Title</label>
        <input
          type="text"
          value={task.title || ''}
          onChange={e => handleChange('title', e.target.value)}
          className="w-full bg-[var(--bg-secondary)] border border-[var(--border)] rounded-lg px-3 h-10 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
        />
      </div>

      <div className="grid grid-cols-2 gap-4">
        <div className="space-y-1.5">
          <label className="text-[10px] font-bold uppercase text-[var(--text-secondary)] px-1">Priority</label>
          <select
            value={task.priority || ''}
            onChange={e => handleChange('priority', e.target.value)}
            className="w-full bg-[var(--bg-secondary)] border border-[var(--border)] rounded-lg px-3 h-10 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
          >
            <option value="1">P1 - High</option>
            <option value="2">P2 - Medium</option>
            <option value="3">P3 - Low</option>
          </select>
        </div>
        <div className="space-y-1.5">
          <label className="text-[10px] font-bold uppercase text-[var(--text-secondary)] px-1">Category</label>
          <input
            type="text"
            value={task.category || ''}
            onChange={e => handleChange('category', e.target.value)}
            className="w-full bg-[var(--bg-secondary)] border border-[var(--border)] rounded-lg px-3 h-10 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
          />
        </div>
      </div>

      <div className="space-y-1.5">
        <label className="text-[10px] font-bold uppercase text-[var(--text-secondary)] px-1">Description</label>
        <textarea
          value={task.description || ''}
          onChange={e => handleChange('description', e.target.value)}
          className="w-full bg-[var(--bg-secondary)] border border-[var(--border)] rounded-lg p-3 min-h-[100px] text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
        />
      </div>

      <div className="space-y-1.5">
        <label className="text-[10px] font-bold uppercase text-[var(--text-secondary)] px-1">Objective</label>
        <input
          type="text"
          value={task.objective || ''}
          onChange={e => handleChange('objective', e.target.value)}
          className="w-full bg-[var(--bg-secondary)] border border-[var(--border)] rounded-lg px-3 h-10 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
        />
      </div>

      <div className="space-y-1.5">
        <label className="text-[10px] font-bold uppercase text-[var(--text-secondary)] px-1">Steps</label>
        <div className="space-y-2">
          {(task.steps || []).map((step, i) => (
            <div key={i} className="flex gap-2">
              <input
                type="text"
                value={step}
                onChange={e => {
                  const newSteps = [...task.steps];
                  newSteps[i] = e.target.value;
                  handleChange('steps', newSteps);
                }}
                className="flex-1 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-lg px-3 h-8 text-xs text-[var(--text-primary)]"
              />
              <button 
                onClick={() => {
                  const newSteps = task.steps.filter((_, index) => index !== i);
                  handleChange('steps', newSteps);
                }}
                className="text-rose-500 hover:text-rose-600 px-1"
              >
                <Icons.XIcon className="h-3 w-3" />
              </button>
            </div>
          ))}
          <Button 
            className="w-full h-8 text-xs border border-dashed border-[var(--border)] bg-transparent shadow-none hover:bg-[var(--bg-hover)]"
            onClick={() => handleChange('steps', [...(task.steps || []), ''])}
          >
            + Add Step
          </Button>
        </div>
      </div>

      <div className="space-y-1.5">
        <label className="text-[10px] font-bold uppercase text-[var(--text-secondary)] px-1">Dependencies (IDs)</label>
        <textarea
          value={(task.dependencies || []).join(', ')}
          onChange={e => handleChange('dependencies', e.target.value.split(',').map(s => s.trim()).filter(Boolean))}
          placeholder="task-1, task-2"
          className="w-full bg-[var(--bg-secondary)] border border-[var(--border)] rounded-lg p-3 min-h-[60px] text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
        />
      </div>

      <div className="space-y-1.5">
        <label className="text-[10px] font-bold uppercase text-[var(--text-secondary)] px-1">Resources</label>
        <textarea
          value={(task.exclusiveResources || []).join(', ')}
          onChange={e => handleChange('exclusiveResources', e.target.value.split(',').map(s => s.trim()).filter(Boolean))}
          placeholder="file/path/1.ts, file/path/2.ts"
          className="w-full bg-[var(--bg-secondary)] border border-[var(--border)] rounded-lg p-3 min-h-[60px] text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
        />
      </div>

      <div className="space-y-1.5">
        <label className="text-[10px] font-bold uppercase text-[var(--text-secondary)] px-1">Notes</label>
        <textarea
          value={task.notes || ''}
          onChange={e => handleChange('notes', e.target.value)}
          className="w-full bg-[var(--bg-secondary)] border border-[var(--border)] rounded-lg p-3 min-h-[80px] text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
        />
      </div>
    </div>
  );
};

export default PrdPage;
