import React from 'react';
import { getConfig, getDoctorReport, getSnapshot, getWorktrees, ApiClientError } from '../api/client';
import type {
  ApiConfigResponse,
  ApiCoordinatorAction,
  ApiCoordinatorCommandResult,
  ApiCoordinatorStatus,
  ApiDoctorReport,
  ApiEventPayload,
  ApiFailureReport,
  ApiRuntimeSnapshot,
  ApiWorktree,
} from '../api/models';
import { Button } from '../components/Button';
import { StatusBadge, type StatusTone } from '../components/StatusBadge';
import { OwnershipBadge } from '../components/OwnershipBadge';
import { TakeoverNotificationToast } from '../components/TakeoverNotificationToast';
import { AlertTriangleIcon, MinusIcon, RefreshIcon, XCircleIcon } from '../components/icons';
import { cn } from '../components/styles';
import { useEventSource } from '../hooks/useEventSource';
import { useCoordinatorStore } from '../store';
import { useIsOwner } from '../stores/ownershipStore';

type NoticeTone = 'success' | 'error';

interface NoticeState {
  tone: NoticeTone;
  message: string;
}

interface CoordinatorActionConfig {
  action: ApiCoordinatorAction;
  label: string;
  description: string;
  emphasis: 'primary' | 'secondary' | 'danger';
}

interface AlertItem {
  id: string;
  title: string;
  detail: string;
  tone: StatusTone;
}

const ACTIONS: CoordinatorActionConfig[] = [
  {
    action: 'run',
    label: 'Run coordinator',
    description: 'Start or continue orchestration with the current queue.',
    emphasis: 'primary',
  },
  {
    action: 'stop',
    label: 'Stop coordinator',
    description: 'Request a stop and refresh the latest coordinator state.',
    emphasis: 'danger',
  },
];

const NOTICE_TIMEOUT_MS = 4500;
const REFRESH_INTERVAL_MS = 10_000;

function formatApiError(error: unknown): string {
  if (error instanceof ApiClientError) {
    return `${error.envelope.error.code}: ${error.envelope.error.message}`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return 'Unexpected coordinator error.';
}

function isOwnershipError(error: unknown): boolean {
  if (!(error instanceof ApiClientError)) return false;
  return error.status === 403;
}

function ownershipErrorMessage(error: unknown): string {
  if (error instanceof ApiClientError && error.status === 403) {
    return 'Viewer mode — request takeover to take control, then retry.';
  }
  return formatApiError(error);
}

function formatResultSummary(action: ApiCoordinatorAction, result: ApiCoordinatorCommandResult): string {
  if (result.selected_task) {
    return `${action} started ${result.selected_task.id}: ${result.selected_task.title}`;
  }
  if (typeof result.runtime_status === 'string' && result.runtime_status.length > 0) {
    return `${action} completed. Runtime status: ${result.runtime_status}.`;
  }
  if (typeof result.removed_worktrees === 'number') {
    return `${action} completed. Removed ${result.removed_worktrees} worktrees.`;
  }
  if (typeof result.aggregated_performer_logs === 'number') {
    return `${action} completed. Aggregated ${result.aggregated_performer_logs} performer logs.`;
  }
  if (typeof result.resumed === 'boolean') {
    return result.resumed ? 'Coordinator resumed.' : 'Coordinator was already active.';
  }
  return `${action} completed successfully.`;
}

function statusLabel(status: ApiCoordinatorStatus | null): string {
  if (!status) return 'Unknown';
  if (status.paused) return 'Paused';
  if (status.active > 0) return 'Running';
  if (status.todo > 0) return 'Idle';
  return 'Complete';
}

function statusTone(status: ApiCoordinatorStatus | null): StatusTone {
  if (!status) return 'todo';
  if (status.paused) return 'paused';
  if (status.blocked > 0) return 'blocked';
  if (status.active > 0) return 'active';
  if (status.todo > 0) return 'todo';
  return 'merged';
}

function failureSummary(report: ApiFailureReport | null): string | null {
  if (!report) return null;
  return `${report.source}: ${report.message}`;
}

function noticeClassName(tone: NoticeTone): string {
  if (tone === 'success') {
    return 'border-[var(--success)]/40 bg-[var(--success)]/15 text-[var(--text-primary)]';
  }
  return 'border-[var(--error)]/40 bg-[var(--error)]/15 text-[var(--text-primary)]';
}

function safeString(value: unknown): string | null {
  return typeof value === 'string' && value.trim().length > 0 ? value.trim() : null;
}

function safeBoolean(value: unknown): boolean | null {
  return typeof value === 'boolean' ? value : null;
}

function pickPathFromStatus(status: ApiCoordinatorStatus | null): string | null {
  if (!status || typeof status !== 'object') return null;
  const data = status as unknown as Record<string, unknown>;
  return (
    safeString(data.project_path) ??
    safeString(data.projectPath) ??
    safeString(data.repo_path) ??
    safeString(data.repoPath)
  );
}

function pickBranchFromStatus(status: ApiCoordinatorStatus | null): string | null {
  if (!status || typeof status !== 'object') return null;
  const data = status as unknown as Record<string, unknown>;
  return (
    safeString(data.current_branch) ??
    safeString(data.currentBranch) ??
    safeString(data.branch)
  );
}

function pickDirtyFromStatus(status: ApiCoordinatorStatus | null): boolean | null {
  if (!status || typeof status !== 'object') return null;
  const data = status as unknown as Record<string, unknown>;
  return safeBoolean(data.dirty) ?? safeBoolean(data.is_dirty) ?? safeBoolean(data.isDirty);
}

function commonPrefixPath(paths: string[]): string | null {
  if (paths.length === 0) return null;
  const splitPaths = paths.map((path) => path.split('/').filter(Boolean));
  const first = splitPaths[0];
  if (!first) return null;
  let index = 0;
  while (index < first.length) {
    const current = first[index];
    if (!splitPaths.every((parts) => parts[index] === current)) break;
    index += 1;
  }
  if (index === 0) return null;
  return `/${first.slice(0, index).join('/')}`;
}

function summarizeEvent(payload: ApiEventPayload): string {
  const candidateKeys = ['msg', 'detail', 'command', 'event', 'state', 'message'] as const;
  for (const key of candidateKeys) {
    const value = payload[key];
    if (typeof value === 'string' && value.trim().length > 0) {
      return value;
    }
  }
  return `${payload.type} (${payload.status})`;
}

function formatTimeOnly(value: string): string {
  const parsed = Date.parse(value);
  if (Number.isNaN(parsed)) return value.slice(-8);
  return new Date(parsed).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

function buildAlerts(status: ApiCoordinatorStatus | null, doctorReport: ApiDoctorReport | null): AlertItem[] {
  const alerts: AlertItem[] = [];

  if (status?.failure_report?.blocking) {
    alerts.push({
      id: 'blocking-failure-report',
      title: 'Blocking failure report',
      detail: `${status.failure_report.source}: ${status.failure_report.message}`,
      tone: 'failed',
    });
  } else if (status?.latest_error) {
    alerts.push({
      id: 'latest-error',
      title: 'Latest coordinator error',
      detail: status.latest_error,
      tone: 'blocked',
    });
  }

  for (const tool of status?.throttled_tools ?? []) {
    alerts.push({
      id: `throttle-${tool.tool_id}-${tool.throttled_until}`,
      title: `Tool throttled: ${tool.tool_id}`,
      detail: `Until ${tool.throttled_until} (${tool.consecutive_count} consecutive throttle events).`,
      tone: 'paused',
    });
  }

  for (const issue of doctorReport?.issues ?? []) {
    if (issue.severity !== 'warning' && issue.severity !== 'error') continue;
    alerts.push({
      id: `doctor-${issue.name}-${issue.target}`,
      title: `Doctor ${issue.severity}: ${issue.name}`,
      detail: issue.message ?? issue.target,
      tone: issue.severity === 'error' ? 'failed' : 'blocked',
    });
  }

  if (status && !status.paused && status.todo > 0 && status.active === 0) {
    alerts.push({
      id: 'stale-backlog',
      title: 'Potential stale backlog',
      detail: `${status.todo} tasks are pending with no active execution.`,
      tone: 'todo',
    });
  }

  return alerts;
}

function alertIcon(tone: StatusTone) {
  if (tone === 'failed') return <XCircleIcon className="mt-0.5 h-3.5 w-3.5 shrink-0" style={{ color: 'var(--error)' }} />;
  if (tone === 'blocked' || tone === 'paused') return <AlertTriangleIcon className="mt-0.5 h-3.5 w-3.5 shrink-0" style={{ color: 'var(--warning)' }} />;
  return <MinusIcon className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[var(--text-muted)]" />;
}

function workerStatusColor(runtimeStatus: string): string {
  const s = runtimeStatus.toLowerCase();
  if (s === 'in_progress' || s === 'running') return 'var(--accent)';
  if (s === 'completed' || s === 'merged') return 'var(--success)';
  if (s === 'failed') return 'var(--error)';
  return 'var(--text-muted)';
}

function worktreeStatusColor(status: string): string {
  const s = status.toLowerCase();
  if (s === 'in_progress' || s === 'locked' || s === 'running' || s === 'active') return 'var(--accent)';
  if (s === 'clean') return 'var(--success)';
  if (s === 'dirty') return 'var(--warning)';
  if (s === 'prunable') return 'var(--error)';
  return 'var(--text-muted)';
}

const Dashboard: React.FC = () => {
  const [notice, setNotice] = React.useState<NoticeState | null>(null);
  const [worktrees, setWorktrees] = React.useState<ApiWorktree[]>([]);
  const [doctorReport, setDoctorReport] = React.useState<ApiDoctorReport | null>(null);
  const [config, setConfig] = React.useState<ApiConfigResponse | null>(null);
  const [snapshot, setSnapshot] = React.useState<ApiRuntimeSnapshot | null>(null);
  const [isLoadingAux, setIsLoadingAux] = React.useState(true);

  const status = useCoordinatorStore((state) => state.status);
  const loadError = useCoordinatorStore((state) => state.loadError);
  const isLoadingStatus = useCoordinatorStore((state) => state.isLoadingStatus);
  const pendingAction = useCoordinatorStore((state) => state.pendingAction);
  const loadStatus = useCoordinatorStore((state) => state.loadStatus);
  const runCoordinatorAction = useCoordinatorStore((state) => state.runAction);
  const isOwner = useIsOwner();

  const { events, connectionState } = useEventSource('/events', { maxEvents: 30 });

  const showNotice = React.useCallback((tone: NoticeTone, message: string) => {
    setNotice({ tone, message });
  }, []);

  React.useEffect(() => {
    if (!notice) return undefined;
    const timeoutId = window.setTimeout(() => setNotice(null), NOTICE_TIMEOUT_MS);
    return () => window.clearTimeout(timeoutId);
  }, [notice]);

  const refreshDashboard = React.useCallback(
    async (signal?: AbortSignal): Promise<void> => {
      try {
        setIsLoadingAux(true);
        await Promise.all([
          loadStatus(signal),
          getWorktrees({ signal }).then((data) => setWorktrees(data)),
          getDoctorReport({ signal }).then((data) => setDoctorReport(data)),
          getConfig({ signal }).then((data) => setConfig(data)),
          getSnapshot({ signal }).then((data) => setSnapshot(data)).catch(() => undefined),
        ]);
      } catch (error) {
        if (error instanceof DOMException && error.name === 'AbortError') return;
        showNotice('error', formatApiError(error));
      } finally {
        setIsLoadingAux(false);
      }
    },
    [loadStatus, showNotice],
  );

  React.useEffect(() => {
    const abortController = new AbortController();
    void refreshDashboard(abortController.signal);
    return () => abortController.abort();
  }, [refreshDashboard]);

  React.useEffect(() => {
    const intervalId = window.setInterval(() => void refreshDashboard(), REFRESH_INTERVAL_MS);
    return () => window.clearInterval(intervalId);
  }, [refreshDashboard]);

  const handleAction = React.useCallback(
    async (action: ApiCoordinatorAction): Promise<void> => {
      try {
        const result = await runCoordinatorAction(action);
        showNotice('success', formatResultSummary(action, result));
        await refreshDashboard();
      } catch (error) {
        if (isOwnershipError(error)) {
          showNotice('error', ownershipErrorMessage(error));
        } else {
          showNotice('error', formatApiError(error));
        }
      }
    },
    [refreshDashboard, runCoordinatorAction, showNotice],
  );

  const summary = failureSummary(status?.failure_report ?? null);
  const currentStatusLabel = statusLabel(status);
  void statusTone(status);
  const isBusy = pendingAction !== null;

  const worktreeMetrics = React.useMemo(() => {
    const total = worktrees.length;
    let active = 0;
    let stale = 0;
    let dirty = 0;
    for (const worktree of worktrees) {
      const state = (worktree.status ?? '').toLowerCase();
      if (state === 'locked' || state === 'running' || state === 'active' || state === 'in_progress') active += 1;
      if (state === 'prunable' || state === 'stale') stale += 1;
      if (state === 'dirty') dirty += 1;
    }
    return { total, active, stale, dirty };
  }, [worktrees]);

  const projectPath =
    snapshot?.project.root ??
    pickPathFromStatus(status) ??
    commonPrefixPath(worktrees.map((entry) => entry.path).filter((value) => value.length > 0)) ??
    null;

  const projectBranch =
    snapshot?.git.current_branch ??
    pickBranchFromStatus(status) ??
    worktrees.find((entry) => entry.path === projectPath)?.branch ??
    worktrees.find((entry) => entry.branch)?.branch ??
    config?.referenceBranch ??
    null;

  const projectDirty = pickDirtyFromStatus(status) ?? (snapshot ? !snapshot.git.clean : worktreeMetrics.dirty > 0);
  const maccVersion = config?.version ?? null;

  const recentEvents = React.useMemo(
    () => events.filter((entry) => entry.stream === 'coordinator_event').slice(0, 12),
    [events],
  );

  const alerts = React.useMemo(() => buildAlerts(status, doctorReport), [doctorReport, status]);

  // Queue stats: prefer snapshot (richer), fallback to status
  const queueStats = React.useMemo(() => {
    if (snapshot) {
      const q = snapshot.queue;
      return [
        { label: 'todo', value: q.todo, color: q.todo > 0 ? 'var(--text-primary)' : 'var(--text-muted)' },
        { label: 'active', value: q.in_progress, color: q.in_progress > 0 ? 'var(--accent)' : 'var(--text-muted)' },
        { label: 'reviewing', value: q.reviewing + q.changes_requested, color: (q.reviewing + q.changes_requested) > 0 ? 'var(--warning)' : 'var(--text-muted)' },
        { label: 'blocked', value: q.blocked, color: q.blocked > 0 ? 'var(--warning)' : 'var(--text-muted)' },
        { label: 'merged', value: q.merged, color: q.merged > 0 ? 'var(--success)' : 'var(--text-muted)' },
        { label: 'workers', value: snapshot.workers.length, color: snapshot.workers.length > 0 ? 'var(--accent)' : 'var(--text-muted)' },
      ];
    }
    if (status) {
      return [
        { label: 'todo', value: status.todo, color: status.todo > 0 ? 'var(--text-primary)' : 'var(--text-muted)' },
        { label: 'active', value: status.active, color: status.active > 0 ? 'var(--accent)' : 'var(--text-muted)' },
        { label: 'reviewing', value: 0, color: 'var(--text-muted)' },
        { label: 'blocked', value: status.blocked, color: status.blocked > 0 ? 'var(--warning)' : 'var(--text-muted)' },
        { label: 'merged', value: status.merged, color: status.merged > 0 ? 'var(--success)' : 'var(--text-muted)' },
        { label: 'worktrees', value: worktreeMetrics.total, color: 'var(--text-muted)' },
      ];
    }
    return null;
  }, [snapshot, status, worktreeMetrics.total]);

  const isRunning = snapshot?.coordinator.running ?? (status ? !status.paused && status.active > 0 : false);
  const isPaused = snapshot?.coordinator.paused ?? status?.paused ?? false;
  const pauseReason = snapshot?.coordinator.pause_reason ?? status?.pause_reason ?? summary;

  // Workers: prefer snapshot workers, fallback to worktrees
  const hasSnapshotWorkers = (snapshot?.workers.length ?? 0) > 0;

  return (
    <section className="relative mx-auto flex w-full max-w-5xl flex-col gap-4 text-[var(--text-primary)]">
      <TakeoverNotificationToast />

      {/* Toast notice */}
      {notice && (
        <div className="pointer-events-none fixed right-4 top-4 z-[var(--z-toast)] w-[min(28rem,calc(100vw-2rem))]">
          <div className={cn('rounded-lg border px-4 py-3 shadow-lg', noticeClassName(notice.tone))}>
            <p className="text-sm font-semibold">
              {notice.tone === 'success' ? 'Done' : 'Error'}
            </p>
            <p className="mt-0.5 text-sm">{notice.message}</p>
          </div>
        </div>
      )}

      {/* Status bar */}
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        {/* Live status indicator */}
        <div className="flex items-center gap-2">
          <span className="relative flex h-2 w-2 shrink-0">
            {isRunning && !isPaused ? (
              <>
                <span
                  className="absolute inline-flex h-full w-full animate-ping rounded-full opacity-60"
                  style={{ backgroundColor: 'var(--success)' }}
                />
                <span
                  className="relative inline-flex h-2 w-2 rounded-full"
                  style={{ backgroundColor: 'var(--success)' }}
                />
              </>
            ) : (
              <span
                className="h-2 w-2 rounded-full"
                style={{ backgroundColor: isPaused ? 'var(--warning)' : 'var(--border)' }}
              />
            )}
          </span>
          <span className="text-sm font-medium">{currentStatusLabel}</span>
        </div>

        {/* Project context */}
        {(projectBranch || projectPath || maccVersion) && (
          <>
            <span className="text-[var(--border)]" aria-hidden>·</span>
            <span className="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
              {projectBranch && (
                <span className="font-mono text-[13px]">{projectBranch}</span>
              )}
              {projectDirty && (
                <span
                  className="rounded px-1.5 py-0.5 text-[10px] font-medium"
                  style={{
                    backgroundColor: 'oklch(0.75 0.17 80 / 0.15)',
                    color: 'var(--warning)',
                  }}
                >
                  dirty
                </span>
              )}
              {maccVersion && (
                <span className="text-[var(--text-muted)]">v{maccVersion}</span>
              )}
            </span>
          </>
        )}

        {/* SSE + controls */}
        <div className="ml-auto flex flex-wrap items-center gap-2">
          <span
            className="text-xs"
            style={{ color: connectionState === 'open' ? 'var(--success)' : 'var(--text-muted)' }}
          >
            {connectionState === 'open' ? '● live' : '○ connecting'}
          </span>

          <OwnershipBadge />

          {ACTIONS.map((actionCfg) => {
            const isPending = pendingAction === actionCfg.action;
            const viewerDisabled = !isOwner;
            return (
              <Button
                key={actionCfg.action}
                className={cn(
                  'h-8 px-3 text-xs',
                  actionCfg.emphasis === 'primary'
                    ? 'border-transparent bg-[var(--accent)] text-white hover:brightness-110'
                    : 'border-transparent bg-[var(--error)] text-white hover:brightness-110',
                )}
                disabled={isBusy || isLoadingStatus || isLoadingAux || viewerDisabled}
                onClick={() => void handleAction(actionCfg.action)}
                title={viewerDisabled ? 'Viewer mode — request takeover to take control' : undefined}
                type="button"
              >
                {isPending ? 'Working...' : actionCfg.label}
              </Button>
            );
          })}

          <Button
            className="h-8 w-8 border-[var(--border)] bg-[var(--bg-card)] p-0 text-[var(--text-muted)] hover:text-[var(--text-primary)]"
            disabled={isLoadingStatus || isLoadingAux || isBusy}
            onClick={() => void refreshDashboard()}
            title="Refresh dashboard"
            type="button"
          >
            <RefreshIcon className={cn('h-3.5 w-3.5', (isLoadingStatus || isLoadingAux) && 'animate-spin')} />
          </Button>
        </div>
      </div>

      {/* Load error */}
      {loadError && (
        <div className="rounded-md border border-[var(--error)]/40 bg-[var(--error)]/10 px-4 py-2.5 text-sm">
          <span className="font-semibold">Unable to load coordinator status.</span>{' '}
          <span className="text-[var(--text-secondary)]">{loadError}</span>
        </div>
      )}

      {/* Pause notice */}
      {isPaused && (
        <div className="rounded-md border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-4 py-2.5 text-sm">
          <span className="font-semibold" style={{ color: 'var(--warning)' }}>Coordinator paused.</span>
          {pauseReason && (
            <span className="ml-2 text-[var(--text-secondary)]">{pauseReason}</span>
          )}
        </div>
      )}

      {/* Alerts */}
      {alerts.length > 0 && (
        <div
          className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
          style={{ boxShadow: 'var(--shadow-soft)' }}
        >
          <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-2.5">
            <span className="text-sm font-semibold text-[var(--text-primary)]">Alerts</span>
            <span className="text-xs text-[var(--text-muted)]">{alerts.length} open</span>
          </div>
          <ul className="divide-y divide-[var(--border-subtle)]">
            {alerts.map((alert) => (
              <li key={alert.id} className="flex items-start gap-3 px-4 py-3">
                {alertIcon(alert.tone)}
                <div className="min-w-0 flex-1">
                  <p className="text-sm font-medium text-[var(--text-primary)]">{alert.title}</p>
                  <p className="mt-0.5 text-sm text-[var(--text-secondary)]">{alert.detail}</p>
                </div>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Queue strip */}
      {queueStats && (
        <div
          className="flex overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
          style={{ boxShadow: 'var(--shadow-soft)' }}
        >
          {queueStats.map((stat, index) => (
            <div
              key={stat.label}
              className={cn(
                'flex min-w-0 flex-1 flex-col items-center py-4',
                index > 0 && 'border-l border-[var(--border)]',
              )}
            >
              <span
                className="text-2xl font-semibold tabular-nums leading-none"
                style={{ color: stat.color }}
              >
                {stat.value}
              </span>
              <span className="mt-1.5 text-[11px] text-[var(--text-muted)]">{stat.label}</span>
            </div>
          ))}
        </div>
      )}

      {/* Body: workers + events */}
      <div className="grid gap-4 xl:grid-cols-2">
        {/* Workers */}
        <div
          className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
          style={{ boxShadow: 'var(--shadow-soft)' }}
        >
          <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-2.5">
            <span className="text-sm font-semibold text-[var(--text-primary)]">Workers</span>
            <span className="text-xs text-[var(--text-muted)]">
              {hasSnapshotWorkers ? snapshot!.workers.length : worktreeMetrics.total} total
            </span>
          </div>

          {hasSnapshotWorkers ? (
            <ul className="divide-y divide-[var(--border-subtle)]">
              {snapshot!.workers.map((worker) => (
                <li key={worker.id} className="flex items-center gap-3 px-4 py-2.5">
                  <span
                    className="h-1.5 w-1.5 shrink-0 rounded-full"
                    style={{ backgroundColor: workerStatusColor(worker.runtime_status) }}
                  />
                  <span className="min-w-0 flex-1 truncate font-mono text-[13px] text-[var(--text-primary)]">
                    {worker.task_id ?? worker.id}
                  </span>
                  <span className="shrink-0 text-xs text-[var(--text-secondary)]">{worker.tool}</span>
                  <span className="shrink-0 text-[11px] text-[var(--text-muted)]">{worker.runtime_status}</span>
                </li>
              ))}
            </ul>
          ) : worktrees.length > 0 ? (
            <ul className="divide-y divide-[var(--border-subtle)]">
              {worktrees.slice(0, 10).map((worktree) => {
                const state = (worktree.status ?? 'unknown').toLowerCase();
                return (
                  <li key={worktree.id} className="flex items-center gap-3 px-4 py-2.5">
                    <span
                      className="h-1.5 w-1.5 shrink-0 rounded-full"
                      style={{ backgroundColor: worktreeStatusColor(state) }}
                    />
                    <span className="min-w-0 flex-1 truncate font-mono text-[13px] text-[var(--text-primary)]">
                      {worktree.id}
                    </span>
                    {worktree.tool && (
                      <span className="shrink-0 text-xs text-[var(--text-secondary)]">{worktree.tool}</span>
                    )}
                    <span className="shrink-0 text-[11px] text-[var(--text-muted)]">{state}</span>
                  </li>
                );
              })}
              {worktrees.length > 10 && (
                <li className="px-4 py-2 text-xs text-[var(--text-muted)]">
                  +{worktrees.length - 10} more worktrees
                </li>
              )}
            </ul>
          ) : (
            <p className="px-4 py-6 text-sm text-[var(--text-secondary)]">
              {isLoadingAux ? 'Loading...' : 'No worktrees found.'}
            </p>
          )}
        </div>

        {/* Events */}
        <div
          className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
          style={{ boxShadow: 'var(--shadow-soft)' }}
        >
          <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-2.5">
            <span className="text-sm font-semibold text-[var(--text-primary)]">Recent events</span>
            <span className="text-xs text-[var(--text-muted)]">
              {connectionState === 'open' ? 'live' : 'disconnected'}
            </span>
          </div>

          {recentEvents.length > 0 ? (
            <ul className="divide-y divide-[var(--border-subtle)]">
              {recentEvents.map((entry) => (
                <li
                  key={`${entry.payload.event_id}-${entry.receivedAt}`}
                  className="flex items-center gap-3 px-4 py-2"
                  title={summarizeEvent(entry.payload)}
                >
                  <span className="w-14 shrink-0 font-mono text-[11px] text-[var(--text-muted)]">
                    {formatTimeOnly(entry.payload.ts)}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-sm text-[var(--text-primary)]">
                    {entry.payload.type}
                  </span>
                  <StatusBadge
                    className="shrink-0 py-0 text-[10px]"
                    status={String(entry.payload.status)}
                    tone={
                      String(entry.payload.status) === 'running' || String(entry.payload.status) === 'started'
                        ? 'active'
                        : String(entry.payload.status) === 'failed'
                          ? 'failed'
                          : String(entry.payload.status) === 'completed' || String(entry.payload.status) === 'merged'
                            ? 'merged'
                            : 'todo'
                    }
                  />
                </li>
              ))}
            </ul>
          ) : (
            <div className="px-4 py-6 text-sm text-[var(--text-secondary)]">
              Waiting for coordinator events.
            </div>
          )}
        </div>
      </div>

      {/* Throttled tools (from snapshot) */}
      {(snapshot?.throttled_tools.length ?? 0) > 0 && (
        <div className="flex flex-wrap gap-2">
          {snapshot!.throttled_tools.map((t) => (
            <span
              key={t.tool}
              className="inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-xs"
              style={{
                borderColor: 'oklch(0.75 0.17 80 / 0.35)',
                backgroundColor: 'oklch(0.75 0.17 80 / 0.1)',
                color: 'var(--warning)',
              }}
            >
              <AlertTriangleIcon className="h-3 w-3" />
              {t.tool} throttled — {t.backoff_seconds}s
            </span>
          ))}
        </div>
      )}
    </section>
  );
};

export default Dashboard;
