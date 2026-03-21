import React, { useState, useEffect, useMemo } from 'react';
import { useNavigate } from 'react-router-dom';
import { 
  useReactTable, 
  getCoreRowModel, 
  getSortedRowModel, 
  getFilteredRowModel,
  flexRender,
  createColumnHelper,
  type SortingState,
} from '@tanstack/react-table';
import { useWorktreeStore } from '../../stores/worktreeStore';
import type { ApiWorktree } from '../../api/models';
import { 
  Button, 
  KpiCard, 
  StatusBadge, 
  type StatusTone, 
  WorktreeCard,
  ConfirmDialog,
  LoadingSpinner,
  ErrorBanner
} from '../../components';
import * as Icons from '../../components/icons';
import { cn } from '../../components/styles';

const columnHelper = createColumnHelper<ApiWorktree>();

const Worktrees: React.FC = () => {
  const navigate = useNavigate();
  const { worktrees, isLoading, error, loadWorktrees, removeWorktree, runWorktree } = useWorktreeStore();
  
  const [viewMode, setViewMode] = useState<'table' | 'cards'>(() => {
    return (localStorage.getItem('macc-worktrees-view') as 'table' | 'cards') || 'cards';
  });
  
  const [globalFilter, setGlobalFilter] = useState('');
  const [sorting, setSorting] = useState<SortingState>([]);
  const [worktreeToRemove, setWorktreeToRemove] = useState<string | null>(null);

  useEffect(() => {
    loadWorktrees();
    const interval = setInterval(() => loadWorktrees(), 30000);
    return () => clearInterval(interval);
  }, [loadWorktrees]);

  useEffect(() => {
    localStorage.setItem('macc-worktrees-view', viewMode);
  }, [viewMode]);

  const kpis = useMemo(() => {
    const total = worktrees.length;
    const active = worktrees.filter(w => w.status === 'active' || w.status === 'running').length;
    const idle = worktrees.filter(w => !w.status || w.status === 'idle' || w.status === 'todo').length;
    // Simple heuristic for stale: if it's been active but not merged and it's old (we don't have timestamps here, so maybe just 0 for now or based on some status)
    const stale = worktrees.filter(w => w.status === 'blocked' || w.status === 'failed').length;
    
    return { total, active, idle, stale };
  }, [worktrees]);

  const filteredWorktrees = useMemo(() => {
    if (!globalFilter) return worktrees;
    const search = globalFilter.toLowerCase();
    return worktrees.filter(w => 
      (w.slug?.toLowerCase().includes(search)) || 
      (w.id.toLowerCase().includes(search)) ||
      (w.branch?.toLowerCase().includes(search)) ||
      (w.tool?.toLowerCase().includes(search))
    );
  }, [worktrees, globalFilter]);

  const getStatusTone = (status: string | null): StatusTone => {
    switch (status?.toLowerCase()) {
      case 'active':
      case 'running':
        return 'active';
      case 'blocked':
        return 'blocked';
      case 'failed':
      case 'error':
        return 'failed';
      case 'merged':
      case 'success':
        return 'merged';
      default:
        return 'todo';
    }
  };

  const columns = useMemo(() => [
    columnHelper.accessor('slug', {
      header: 'Name',
      cell: info => <span className="font-bold">{info.getValue() || info.row.original.id}</span>,
      size: 200,
    }),
    columnHelper.accessor('branch', {
      header: 'Branch',
      cell: info => (
        <div className="flex items-center gap-2 text-[var(--text-secondary)]">
          <Icons.BranchIcon className="h-3.5 w-3.5" />
          <span className="truncate max-w-[150px]">{info.getValue() || '-'}</span>
        </div>
      ),
      size: 180,
    }),
    columnHelper.accessor('tool', {
      header: 'Tool',
      cell: info => <span className="text-xs font-mono uppercase opacity-70">{info.getValue() || '-'}</span>,
      size: 100,
    }),
    columnHelper.accessor('status', {
      header: 'Status',
      cell: info => <StatusBadge status={info.getValue() || 'unknown'} tone={getStatusTone(info.getValue())} />,
      size: 120,
    }),
    columnHelper.accessor('scope', {
      header: 'Scope',
      cell: info => <span className="text-xs text-[var(--text-muted)] italic">{info.getValue() || '-'}</span>,
      size: 100,
    }),
    columnHelper.display({
      id: 'actions',
      header: 'Actions',
      cell: info => (
        <div className="flex items-center gap-2">
          <Button className="p-1 h-8 w-8 bg-transparent border-none hover:bg-white/10" onClick={() => runWorktree(info.row.original.id)} title="Run">
            <Icons.PlayIcon className="h-4 w-4" />
          </Button>
          <Button className="p-1 h-8 w-8 bg-transparent border-none hover:bg-white/10 text-[var(--accent)]" onClick={() => navigate('/ops/diagnostics')} title="Doctor">
            <Icons.ActivityIcon className="h-4 w-4" />
          </Button>
          <Button className="p-1 h-8 w-8 bg-transparent border-none hover:bg-white/10 text-rose-500" onClick={() => setWorktreeToRemove(info.row.original.id)} title="Remove">
            <Icons.TrashIcon className="h-4 w-4" />
          </Button>
        </div>
      ),
      size: 120,
    }),
  ], [runWorktree]);

  const table = useReactTable({
    data: filteredWorktrees,
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

  const handleConfirmRemove = async () => {
    if (worktreeToRemove) {
      await removeWorktree(worktreeToRemove);
      setWorktreeToRemove(null);
    }
  };

  const handleExport = () => {
    const blob = new Blob([JSON.stringify(worktrees, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `macc-worktrees-${new Date().toISOString()}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="flex flex-col gap-6 p-6">
      <header className="flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-3xl font-bold tracking-tight text-[var(--text-primary)]">Worktrees</h1>
            <p className="text-[var(--text-secondary)]">Manage isolated development environments</p>
          </div>
          <div className="flex items-center gap-3">
            <Button onClick={() => loadWorktrees()} className="gap-2 h-9 bg-transparent border-none hover:bg-white/10">
              <Icons.RefreshIcon className={cn("h-4 w-4", isLoading && "animate-spin")} />
              Refresh
            </Button>
            <Button onClick={handleExport} className="gap-2 h-9 bg-transparent border-none hover:bg-white/10">
              <Icons.DownloadIcon className="h-4 w-4" />
              Export
            </Button>
            <Button onClick={() => navigate('/ops/worktrees/create')} className="gap-2 h-9">
              <Icons.PlusIcon className="h-4 w-4" />
              Create Worktree
            </Button>
          </div>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
          <KpiCard title="Total Worktrees" value={kpis.total} icon={<Icons.FolderIcon className="h-5 w-5" />} />
          <KpiCard title="Active Leases" value={kpis.active} icon={<Icons.ActivityIcon className="h-5 w-5" />} />
          <KpiCard title="Idle Environments" value={kpis.idle} icon={<Icons.ClockIcon className="h-5 w-5" />} />
          <KpiCard title="Stale / Blocked" value={kpis.stale} icon={<Icons.AlertTriangleIcon className="h-5 w-5" />} />
        </div>
      </header>

      <div className="flex items-center justify-between gap-4">
        <div className="relative flex-1 max-w-md">
          <Icons.SearchIcon className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-[var(--text-muted)]" />
          <input
            type="text"
            placeholder="Filter worktrees..."
            value={globalFilter}
            onChange={(e) => setGlobalFilter(e.target.value)}
            className="w-full pl-10 pr-4 py-2 rounded-xl border border-[var(--border)] bg-[var(--bg-card)] text-sm focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/50 transition-all"
          />
        </div>

        <div className="flex items-center bg-[var(--bg-secondary)] p-1 rounded-lg border border-[var(--border)]">
          <button
            onClick={() => setViewMode('table')}
            className={cn(
              "px-3 py-1.5 rounded-md text-sm font-medium transition-all flex items-center gap-2",
              viewMode === 'table' ? "bg-[var(--bg-card)] text-[var(--accent)] shadow-sm" : "text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
            )}
          >
            <Icons.TableIcon className="h-4 w-4" />
            Table
          </button>
          <button
            onClick={() => setViewMode('cards')}
            className={cn(
              "px-3 py-1.5 rounded-md text-sm font-medium transition-all flex items-center gap-2",
              viewMode === 'cards' ? "bg-[var(--bg-card)] text-[var(--accent)] shadow-sm" : "text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
            )}
          >
            <Icons.LayoutGridIcon className="h-4 w-4" />
            Cards
          </button>
        </div>
      </div>

      {error && <ErrorBanner message={error} />}

      {isLoading && worktrees.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-20">
          <LoadingSpinner size="lg" />
          <p className="mt-4 text-[var(--text-muted)] animate-pulse">Loading worktrees...</p>
        </div>
      ) : filteredWorktrees.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-20 border-2 border-dashed border-[var(--border)] rounded-3xl">
          <Icons.FolderOpenIcon className="h-12 w-12 text-[var(--text-muted)] mb-4 opacity-20" />
          <h3 className="text-lg font-medium text-[var(--text-secondary)]">No worktrees found</h3>
          <p className="text-[var(--text-muted)] text-sm">Try adjusting your filters or create a new one.</p>
        </div>
      ) : viewMode === 'table' ? (
        <div className="rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] overflow-hidden shadow-sm">
          <table className="w-full text-left text-sm">
            <thead className="bg-[var(--bg-secondary)] border-b border-[var(--border)]">
              {table.getHeaderGroups().map(headerGroup => (
                <tr key={headerGroup.id}>
                  {headerGroup.headers.map(header => (
                    <th 
                      key={header.id}
                      className="px-6 py-4 font-semibold text-[var(--text-secondary)]"
                      style={{ width: header.getSize() }}
                    >
                      {flexRender(header.column.columnDef.header, header.getContext())}
                    </th>
                  ))}
                </tr>
              ))}
            </thead>
            <tbody>
              {table.getRowModel().rows.map(row => (
                <tr key={row.id} className="border-b border-[var(--border)] last:border-0 hover:bg-[var(--bg-hover)] transition-colors group">
                  {row.getVisibleCells().map(cell => (
                    <td key={cell.id} className="px-6 py-4 align-middle">
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4 gap-6">
          {filteredWorktrees.map(w => (
            <WorktreeCard 
              key={w.id} 
              worktree={w} 
              onRun={runWorktree}
              onDoctor={() => navigate('/ops/diagnostics')}
              onRemove={setWorktreeToRemove}
              onOpen={(id) => navigate(`/ops/worktrees/${id}`)}
            />
          ))}
        </div>
      )}

      <ConfirmDialog
        open={!!worktreeToRemove}
        onOpenChange={(open) => !open && setWorktreeToRemove(null)}
        title="Remove Worktree"
        description="Are you sure you want to remove this worktree? This will delete the local directory and all uncommitted changes. This action cannot be undone."
        confirmLabel="Remove"
        intent="danger"
        onConfirm={handleConfirmRemove}
      />
    </div>
  );
};

export default Worktrees;
