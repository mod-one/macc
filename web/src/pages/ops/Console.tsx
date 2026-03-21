import React, { useCallback, useEffect, useState, useMemo } from 'react';
import { useCoordinatorStore } from '../../store';
import { getRegistryTasks, ApiClientError } from '../../api/client';
import type { 
  ApiCoordinatorAction, 
  ApiCoordinatorStatus, 
  ApiRegistryTask,
  ApiThrottledToolStatus
} from '../../api/models';
import { useEventSource } from '../../hooks/useEventSource';
import { StreamTile } from '../../components/StreamTile';
import { TaskListItem } from '../../components/TaskListItem';
import { StatusBadge, type StatusTone } from '../../components/StatusBadge';
import { Button } from '../../components/Button';
import { ConfirmDialog } from '../../components/ConfirmDialog';
import { Icons } from '../../components/NavIcons';
import { cn } from '../../components/styles';

// --- Types ---

interface ActiveWorktree {
  id: string;
  tool: string;
  status: string;
  logs: string[];
}

// --- Helpers ---

function getStatusTone(status: string | undefined): StatusTone {
  if (!status) return 'todo';
  const s = status.toLowerCase();
  if (s === 'running' || s === 'active') return 'active';
  if (s === 'complete' || s === 'success' || s === 'merged') return 'success';
  if (s === 'failed' || s === 'error') return 'error';
  if (s === 'blocked') return 'warning';
  return 'todo';
}

function formatElapsedTime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  return [h, m, s].map(v => v.toString().padStart(2, '0')).join(':');
}

// --- Components ---

const ToolCooldownPanel: React.FC<{
  throttledTools: ApiThrottledToolStatus[];
  onClear: (toolId: string) => void;
  isBusy: boolean;
}> = ({ throttledTools, onClear, isBusy }) => {
  if (throttledTools.length === 0) {
    return (
      <div className="rounded-xl border border-dashed border-[var(--border)] p-8 text-center">
        <p className="text-sm text-[var(--text-muted)]">No active tool cooldowns.</p>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {throttledTools.map((tool) => (
        <div 
          key={tool.tool_id}
          className="flex items-center justify-between rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4"
        >
          <div className="space-y-1">
            <div className="flex items-center gap-2">
              <span className="font-semibold text-[var(--text-primary)]">{tool.tool_id}</span>
              <span className="rounded-full bg-rose-500/10 px-2 py-0.5 text-[0.65rem] font-bold uppercase tracking-wider text-rose-500">
                Throttled
              </span>
            </div>
            <p className="text-xs text-[var(--text-secondary)]">
              Until: {new Date(tool.throttled_until).toLocaleTimeString()} ({tool.consecutive_count} events)
            </p>
          </div>
          <Button
            size="sm"
            variant="secondary"
            disabled={isBusy}
            onClick={() => onClear(tool.tool_id)}
          >
            Clear
          </Button>
        </div>
      ))}
    </div>
  );
};

const Console: React.FC = () => {
  // --- State & Store ---
  const status = useCoordinatorStore((state) => state.status);
  const loadStatus = useCoordinatorStore((state) => state.loadStatus);
  const runAction = useCoordinatorStore((state) => state.runAction);
  const pendingAction = useCoordinatorStore((state) => state.pendingAction);
  
  const [tasks, setTasks] = useState<ApiRegistryTask[]>([]);
  const [isLoadingTasks, setIsLoadingTasks] = useState(false);
  const [startTime] = useState<number>(Date.now());
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [activeWorktrees, setActiveWorktrees] = useState<Record<string, ActiveWorktree>>({});
  const [showEmergencyStop, setShowEmergencyStop] = useState(false);

  // --- Real-time Events ---
  const { events } = useEventSource('/coordinator_event', { maxEvents: 100 });

  // --- Calculations ---
  const healthPercent = useMemo(() => {
    if (!status || status.total === 0) return 0;
    return Math.round((status.merged / status.total) * 100);
  }, [status]);

  const sortedTasks = useMemo(() => {
    return [...tasks].sort((a, b) => {
      // Sort active/in_progress first
      if (a.state === 'in_progress' && b.state !== 'in_progress') return -1;
      if (a.state !== 'in_progress' && b.state === 'in_progress') return 1;
      // Then by updatedAt
      return new Date(b.updatedAt || 0).getTime() - new Date(a.updatedAt || 0).getTime();
    });
  }, [tasks]);

  // --- Effects ---
  
  // Refresh status and tasks periodically
  useEffect(() => {
    const fetchAll = async () => {
      setIsLoadingTasks(true);
      try {
        await loadStatus();
        const registryTasks = await getRegistryTasks();
        setTasks(registryTasks);
      } catch (err) {
        console.error('Failed to fetch console data:', err);
      } finally {
        setIsLoadingTasks(false);
      }
    };

    fetchAll();
    const interval = setInterval(fetchAll, 5000);
    return () => clearInterval(interval);
  }, [loadStatus]);

  // Update elapsed time
  useEffect(() => {
    const interval = setInterval(() => {
      setElapsedSeconds(Math.floor((Date.now() - startTime) / 1000));
    }, 1000);
    return () => clearInterval(interval);
  }, [startTime]);

  // Process incoming events to update active worktree logs
  useEffect(() => {
    if (events.length === 0) return;
    
    // We only care about new events since last update
    const latestEvent = events[0];
    const payload = latestEvent.payload;
    
    if (payload.type === 'performer_log' && typeof payload.line === 'string') {
      const taskId = payload.task_id as string;
      const worktreePath = payload.worktree as string;
      
      if (taskId && worktreePath) {
        setActiveWorktrees(prev => {
          const existing = prev[taskId] || {
            id: taskId,
            tool: (payload.tool as string) || 'unknown',
            status: 'active',
            logs: []
          };
          
          return {
            ...prev,
            [taskId]: {
              ...existing,
              logs: [payload.line as string, ...existing.logs].slice(0, 50)
            }
          };
        });
      }
    } else if (payload.type === 'task_transition') {
      const taskId = payload.task_id as string;
      const nextState = payload.status as string;
      
      if (nextState === 'success' || nextState === 'failed' || nextState === 'merged') {
        // Task finished, remove from active view after a delay or immediately
        setActiveWorktrees(prev => {
          const next = { ...prev };
          delete next[taskId];
          return next;
        });
      }
    }
  }, [events]);

  // --- Handlers ---

  const handleAction = async (action: ApiCoordinatorAction) => {
    try {
      await runAction(action);
    } catch (err) {
      console.error(`Action ${action} failed:`, err);
    }
  };

  const handleEmergencyStop = async () => {
    setShowEmergencyStop(false);
    await handleAction('stop');
  };

  const clearCooldown = async (toolId: string) => {
    // Reconcile is the action to clear/refresh coordinator state including cooldowns
    await handleAction('reconcile');
  };

  const isBusy = pendingAction !== null;

  return (
    <div className="flex flex-col gap-8 pb-12">
      {/* --- Header & KPIs --- */}
      <section className="space-y-6">
        <header className="flex flex-col gap-6 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <div className="flex items-center gap-3">
              <h1 className="text-4xl font-bold tracking-tight text-[var(--text-primary)]">
                Coordinator Console
              </h1>
              <div className="flex gap-2">
                {isBusy && (
                  <span className="flex items-center gap-1.5 rounded-full bg-sky-500/10 px-3 py-1 text-xs font-bold uppercase tracking-wider text-sky-500">
                    <Icons.RefreshIcon className="h-3 w-3 animate-spin" />
                    Syncing
                  </span>
                )}
                <StatusBadge 
                  status={status?.paused ? 'Paused' : (status?.active ?? 0) > 0 ? 'Running' : 'Idle'} 
                  tone={status?.paused ? 'warning' : (status?.active ?? 0) > 0 ? 'active' : 'todo'}
                />
                {status?.throttled_tools && status.throttled_tools.length > 0 && (
                  <span className="rounded-full bg-rose-500/10 px-3 py-1 text-xs font-bold uppercase tracking-wider text-rose-500">
                    {status.throttled_tools.length} Throttled
                  </span>
                )}
              </div>
            </div>
            <p className="mt-2 text-[var(--text-secondary)]">
              Real-time orchestration monitoring and manual control-plane override.
            </p>
          </div>

          <div className="flex flex-wrap gap-3">
            <Button
              variant="danger"
              size="lg"
              className="font-bold shadow-lg shadow-rose-500/20"
              onClick={() => setShowEmergencyStop(true)}
              disabled={isBusy}
            >
              <Icons.AlertTriangleIcon className="mr-2 h-5 w-5" />
              Emergency Stop
            </Button>
          </div>
        </header>

        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          <KpiCard label="Status" value={status ? (status.paused ? 'PAUSED' : 'ACTIVE') : 'OFFLINE'} tone={status?.paused ? 'warning' : 'accent'} />
          <KpiCard label="Elapsed Time" value={formatElapsedTime(elapsedSeconds)} />
          <KpiCard label="Queue Depth" value={status?.todo ?? 0} tone="accent" />
          <KpiCard label="Health %" value={`${healthPercent}%`} tone={healthPercent > 80 ? 'accent' : healthPercent > 50 ? 'warning' : 'default'} />
        </div>
      </section>

      <div className="grid gap-8 lg:grid-cols-[1fr_360px]">
        {/* --- Main Content --- */}
        <div className="space-y-10">
          {/* Active Performers */}
          <section className="space-y-4">
            <div className="flex items-center justify-between">
              <h2 className="text-xl font-bold text-[var(--text-primary)]">Active Performers</h2>
              <span className="text-sm text-[var(--text-muted)]">{Object.keys(activeWorktrees).length} active</span>
            </div>
            {Object.keys(activeWorktrees).length > 0 ? (
              <div className="grid gap-4 md:grid-cols-2">
                {Object.values(activeWorktrees).map((wt) => (
                  <StreamTile 
                    key={wt.id}
                    title={wt.id}
                    tool={wt.tool}
                    status={wt.status}
                    liveLogTail={wt.logs}
                    statusTone="active"
                  />
                ))}
              </div>
            ) : (
              <div className="rounded-2xl border border-dashed border-[var(--border)] bg-[var(--bg-secondary)]/30 py-12 text-center">
                <Icons.Home className="mx-auto h-8 w-8 text-[var(--text-muted)] opacity-20" />
                <p className="mt-4 text-sm text-[var(--text-muted)]">No active performers at the moment.</p>
              </div>
            )}
          </section>

          {/* Task Registry */}
          <section className="space-y-4">
            <div className="flex items-center justify-between">
              <h2 className="text-xl font-bold text-[var(--text-primary)]">Task Registry</h2>
              <Button variant="secondary" size="sm" onClick={() => handleAction('sync')}>
                Sync Registry
              </Button>
            </div>
            <div className="grid gap-3 overflow-hidden">
              {isLoadingTasks && tasks.length === 0 ? (
                <div className="py-12 text-center">
                  <Icons.RefreshIcon className="mx-auto h-6 w-6 animate-spin text-[var(--text-muted)]" />
                </div>
              ) : tasks.length > 0 ? (
                sortedTasks.slice(0, 10).map((task) => (
                  <TaskListItem 
                    key={task.id}
                    taskId={task.id}
                    title={task.title || 'Untitled Task'}
                    state={task.state}
                    stateTone={getStatusTone(task.state)}
                    tool={task.tool || 'none'}
                    attempts={task.attempts || 0}
                    priority={1}
                  />
                ))
              ) : (
                <p className="py-8 text-center text-sm text-[var(--text-muted)]">Registry is empty.</p>
              )}
              {tasks.length > 10 && (
                <Button variant="ghost" className="w-full text-[var(--text-muted)]">
                  View all tasks in Registry →
                </Button>
              )}
            </div>
          </section>
        </div>

        {/* --- Sidebar Panels --- */}
        <aside className="space-y-8">
          {/* Quick Actions */}
          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] p-6 shadow-sm">
            <h3 className="mb-4 text-sm font-bold uppercase tracking-wider text-[var(--text-muted)]">Quick Actions</h3>
            <div className="grid grid-cols-2 gap-2">
              <ActionButton label="Run" icon={<Icons.PlayIcon className="h-4 w-4" />} onClick={() => handleAction('run')} disabled={isBusy} primary />
              <ActionButton label="Stop" icon={<Icons.XCircleIcon className="h-4 w-4" />} onClick={() => handleAction('stop')} disabled={isBusy} />
              <ActionButton label="Resume" icon={<Icons.RefreshIcon className="h-4 w-4" />} onClick={() => handleAction('resume')} disabled={isBusy} />
              <ActionButton label="Dispatch" icon={<Icons.ArrowUpIcon className="h-4 w-4" />} onClick={() => handleAction('dispatch')} disabled={isBusy} />
              <ActionButton label="Advance" icon={<Icons.ChevronRight className="h-4 w-4" />} onClick={() => handleAction('advance')} disabled={isBusy} />
              <ActionButton label="Reconcile" icon={<Icons.RefreshIcon className="h-4 w-4" />} onClick={() => handleAction('reconcile')} disabled={isBusy} />
              <ActionButton label="Cleanup" icon={<Icons.TrashIcon className="h-4 w-4" />} onClick={() => handleAction('cleanup')} disabled={isBusy} />
              <ActionButton label="Audit PRD" icon={<Icons.SearchIcon className="h-4 w-4" />} onClick={() => handleAction('audit-prd')} disabled={isBusy} />
            </div>
          </section>

          {/* Resources & Cooldowns */}
          <section className="space-y-6 rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] p-6 shadow-sm">
            <div>
              <h3 className="mb-4 text-sm font-bold uppercase tracking-wider text-[var(--text-muted)]">Resource Limits</h3>
              <div className="space-y-4">
                <div className="space-y-2">
                  <div className="flex justify-between text-xs font-medium">
                    <span className="text-[var(--text-secondary)]">Max Parallelism</span>
                    <span className="text-[var(--text-primary)]">{status?.active ?? 0} / {status?.effective_max_parallel ?? '∞'}</span>
                  </div>
                  <div className="h-2 w-full overflow-hidden rounded-full bg-[var(--bg-secondary)]">
                    <div 
                      className="h-full bg-[var(--accent)] transition-all duration-500" 
                      style={{ width: `${Math.min(100, ((status?.active ?? 0) / (status?.effective_max_parallel ?? 1)) * 100)}%` }}
                    />
                  </div>
                </div>
              </div>
            </div>

            <div className="pt-4 border-t border-[var(--border)]">
              <h3 className="mb-4 text-sm font-bold uppercase tracking-wider text-[var(--text-muted)]">Active Cooldowns</h3>
              <ToolCooldownPanel 
                throttledTools={status?.throttled_tools || []} 
                onClear={clearCooldown}
                isBusy={isBusy}
              />
            </div>
          </section>
        </aside>
      </div>

      <ConfirmDialog
        isOpen={showEmergencyStop}
        title="Emergency Stop"
        message="This will immediately request the coordinator to halt all active operations. Performers already in flight will finish their current step. Are you sure?"
        confirmLabel="Stop Everything"
        cancelLabel="Cancel"
        onConfirm={handleEmergencyStop}
        onCancel={() => setShowEmergencyStop(false)}
        variant="danger"
      />
    </div>
  );
};

// --- Sub-components ---

const KpiCard: React.FC<{ label: string; value: string | number; tone?: 'default' | 'accent' | 'warning' }> = ({ label, value, tone = 'default' }) => (
  <article className={cn(
    "rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] p-5 shadow-sm",
    tone === 'accent' && "border-[var(--accent)]/30 bg-[var(--accent)]/[0.02]",
    tone === 'warning' && "border-amber-500/30 bg-amber-500/[0.02]"
  )}>
    <p className="text-xs font-bold uppercase tracking-wider text-[var(--text-muted)]">{label}</p>
    <p className={cn(
      "mt-3 text-3xl font-bold tracking-tight",
      tone === 'accent' ? "text-[var(--accent)]" : tone === 'warning' ? "text-amber-500" : "text-[var(--text-primary)]"
    )}>
      {value}
    </p>
  </article>
);

const ActionButton: React.FC<{ 
  label: string; 
  icon: React.ReactNode; 
  onClick: () => void; 
  disabled?: boolean;
  primary?: boolean;
}> = ({ label, icon, onClick, disabled, primary }) => (
  <button
    onClick={onClick}
    disabled={disabled}
    className={cn(
      "flex flex-col items-center justify-center gap-2 rounded-xl border p-3 text-center transition-all",
      "hover:scale-[1.02] active:scale-[0.98] disabled:opacity-50 disabled:hover:scale-100",
      primary 
        ? "border-[var(--accent)] bg-[var(--accent)] text-white shadow-lg shadow-[var(--accent)]/20 hover:bg-[var(--accent-hover)]" 
        : "border-[var(--border)] bg-[var(--bg-secondary)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)]",
      "font-medium"
    )}
  >
    {icon}
    <span className="text-xs">{label}</span>
  </button>
);

export default Console;
