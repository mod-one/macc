import React from 'react';
import { getConfig, getDoctorReport, getWorktrees, ApiClientError } from '../api/client';
import type {
  ApiConfigResponse,
  ApiCoordinatorAction,
  ApiCoordinatorCommandResult,
  ApiCoordinatorStatus,
  ApiDoctorReport,
  ApiEventPayload,
  ApiFailureReport,
  ApiWorktree,
} from '../api/models';
import { Button } from '../components/Button';
import { KpiCard } from '../components/KpiCard';
import { StatusBadge, type StatusTone } from '../components/StatusBadge';
import { useEventSource } from '../hooks/useEventSource';
import { useCoordinatorStore } from '../store';

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
  if (!status) {
    return 'Unknown';
  }
  if (status.paused) {
    return 'Paused';
  }
  if (status.active > 0) {
    return 'Running';
  }
  if (status.todo > 0) {
    return 'Idle';
  }
  return 'Complete';
}

function statusTone(status: ApiCoordinatorStatus | null): StatusTone {
  if (!status) {
    return 'todo';
  }
  if (status.paused) {
    return 'paused';
  }
  if (status.blocked > 0) {
    return 'blocked';
  }
  if (status.active > 0) {
    return 'active';
  }
  if (status.todo > 0) {
    return 'todo';
  }
  return 'merged';
}

function failureSummary(report: ApiFailureReport | null): string | null {
  if (!report) {
    return null;
  }
  return `${report.source}: ${report.message}`;
}

function actionClassName(emphasis: CoordinatorActionConfig['emphasis']): string {
  if (emphasis === 'primary') {
    return 'border-transparent bg-[var(--accent)] text-white hover:brightness-110';
  }
  if (emphasis === 'danger') {
    return 'border-transparent bg-[var(--error)] text-white hover:brightness-110';
  }
  return 'border-[var(--border)] bg-[var(--bg-secondary)] text-[var(--text-primary)] hover:bg-white/10';
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
  if (!status || typeof status !== 'object') {
    return null;
  }
  const data = status as unknown as Record<string, unknown>;
  return (
    safeString(data.project_path) ??
    safeString(data.projectPath) ??
    safeString(data.repo_path) ??
    safeString(data.repoPath)
  );
}

function pickBranchFromStatus(status: ApiCoordinatorStatus | null): string | null {
  if (!status || typeof status !== 'object') {
    return null;
  }
  const data = status as unknown as Record<string, unknown>;
  return (
    safeString(data.current_branch) ??
    safeString(data.currentBranch) ??
    safeString(data.branch)
  );
}

function pickDirtyFromStatus(status: ApiCoordinatorStatus | null): boolean | null {
  if (!status || typeof status !== 'object') {
    return null;
  }
  const data = status as unknown as Record<string, unknown>;
  return safeBoolean(data.dirty) ?? safeBoolean(data.is_dirty) ?? safeBoolean(data.isDirty);
}

function commonPrefixPath(paths: string[]): string | null {
  if (paths.length === 0) {
    return null;
  }
  const splitPaths = paths.map((path) => path.split('/').filter(Boolean));
  const first = splitPaths[0];
  if (!first) {
    return null;
  }
  let index = 0;
  while (index < first.length) {
    const current = first[index];
    if (!splitPaths.every((parts) => parts[index] === current)) {
      break;
    }
    index += 1;
  }
  if (index === 0) {
    return null;
  }
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

function formatTimestamp(value: string): string {
  const parsed = Date.parse(value);
  if (Number.isNaN(parsed)) {
    return value;
  }
  return new Date(parsed).toLocaleString();
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
    if (issue.severity !== 'warning' && issue.severity !== 'error') {
      continue;
    }
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

const Dashboard: React.FC = () => {
  const [notice, setNotice] = React.useState<NoticeState | null>(null);
  const [worktrees, setWorktrees] = React.useState<ApiWorktree[]>([]);
  const [doctorReport, setDoctorReport] = React.useState<ApiDoctorReport | null>(null);
  const [config, setConfig] = React.useState<ApiConfigResponse | null>(null);
  const [isLoadingAux, setIsLoadingAux] = React.useState(true);

  const status = useCoordinatorStore((state) => state.status);
  const loadError = useCoordinatorStore((state) => state.loadError);
  const isLoadingStatus = useCoordinatorStore((state) => state.isLoadingStatus);
  const pendingAction = useCoordinatorStore((state) => state.pendingAction);
  const loadStatus = useCoordinatorStore((state) => state.loadStatus);
  const runCoordinatorAction = useCoordinatorStore((state) => state.runAction);

  const { events, connectionState } = useEventSource('/events', { maxEvents: 30 });

  const showNotice = React.useCallback((tone: NoticeTone, message: string) => {
    setNotice({ tone, message });
  }, []);

  React.useEffect(() => {
    if (!notice) {
      return undefined;
    }
    const timeoutId = window.setTimeout(() => {
      setNotice(null);
    }, NOTICE_TIMEOUT_MS);
    return () => {
      window.clearTimeout(timeoutId);
    };
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
        ]);
      } catch (error) {
        if (error instanceof DOMException && error.name === 'AbortError') {
          return;
        }
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
    return () => {
      abortController.abort();
    };
  }, [refreshDashboard]);

  React.useEffect(() => {
    const intervalId = window.setInterval(() => {
      void refreshDashboard();
    }, REFRESH_INTERVAL_MS);
    return () => {
      window.clearInterval(intervalId);
    };
  }, [refreshDashboard]);

  const handleAction = React.useCallback(
    async (action: ApiCoordinatorAction): Promise<void> => {
      try {
        const result = await runCoordinatorAction(action);
        showNotice('success', formatResultSummary(action, result));
        await refreshDashboard();
      } catch (error) {
        showNotice('error', formatApiError(error));
      }
    },
    [refreshDashboard, runCoordinatorAction, showNotice],
  );

  const summary = failureSummary(status?.failure_report ?? null);
  const currentStatusLabel = statusLabel(status);
  const currentStatusTone = statusTone(status);
  const isBusy = pendingAction !== null;

  const worktreeMetrics = React.useMemo(() => {
    const total = worktrees.length;
    let active = 0;
    let stale = 0;
    let dirty = 0;
    for (const worktree of worktrees) {
      const state = (worktree.status ?? '').toLowerCase();
      if (state === 'locked' || state === 'running' || state === 'active') {
        active += 1;
      }
      if (state === 'prunable' || state === 'stale') {
        stale += 1;
      }
      if (state === 'dirty') {
        dirty += 1;
      }
    }
    const idle = Math.max(total - active - stale, 0);
    return { total, active, idle, stale, dirty };
  }, [worktrees]);

  const projectPath =
    pickPathFromStatus(status) ??
    commonPrefixPath(worktrees.map((entry) => entry.path).filter((value) => value.length > 0)) ??
    'Unavailable';
  const projectBranch =
    pickBranchFromStatus(status) ??
    worktrees.find((entry) => entry.path === projectPath)?.branch ??
    worktrees.find((entry) => entry.branch)?.branch ??
    config?.referenceBranch ??
    'Unknown';
  const projectDirty = pickDirtyFromStatus(status) ?? worktreeMetrics.dirty > 0;
  const maccVersion = config?.version ?? 'Unknown';

  const recentEvents = React.useMemo(
    () => events.filter((entry) => entry.stream === 'coordinator_event').slice(0, 5),
    [events],
  );

  const alerts = React.useMemo(() => buildAlerts(status, doctorReport), [doctorReport, status]);

  return (
    <section className="relative mx-auto flex w-full max-w-7xl flex-col gap-6 text-[var(--text-primary)]">
      {notice ? (
        <div className="pointer-events-none fixed right-4 top-4 z-20 w-[min(30rem,calc(100vw-2rem))]">
          <div className={`rounded-2xl border px-4 py-3 shadow-lg ${noticeClassName(notice.tone)}`}>
            <p className="text-sm font-semibold">
              {notice.tone === 'success' ? 'Coordinator updated' : 'Action failed'}
            </p>
            <p className="mt-1 text-sm leading-6">{notice.message}</p>
          </div>
        </div>
      ) : null}

      <header className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[radial-gradient(circle_at_top_left,_rgba(59,130,246,0.25),_transparent_35%),var(--bg-secondary)] p-6 shadow-[var(--shadow-soft)]">
        <div className="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
          <div>
            <p className="text-sm font-semibold uppercase tracking-[0.2em] text-[var(--text-secondary)]">
              Operator overview
            </p>
            <h1 className="mt-2 text-4xl font-semibold tracking-tight">Dashboard</h1>
            <p className="mt-2 max-w-2xl text-sm text-[var(--text-secondary)]">
              Primary coordinator summary with project context, live activity, and alerts.
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <StatusBadge status={currentStatusLabel} tone={currentStatusTone} />
            <StatusBadge
              status={connectionState === 'open' ? 'SSE live' : connectionState}
              tone={connectionState === 'open' ? 'active' : 'todo'}
            />
            <Button
              className="border-[var(--border)] bg-[var(--bg-card)] text-[var(--text-primary)] hover:bg-white/10"
              disabled={isLoadingStatus || isLoadingAux || isBusy}
              onClick={() => {
                void refreshDashboard();
              }}
              type="button"
            >
              {isLoadingStatus || isLoadingAux ? 'Refreshing...' : 'Refresh now'}
            </Button>
          </div>
        </div>
      </header>

      <section className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-5 shadow-[var(--shadow-soft)]">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-xl font-semibold">Project summary</h2>
          <StatusBadge status={projectDirty ? 'Dirty' : 'Clean'} tone={projectDirty ? 'blocked' : 'merged'} />
        </div>
        <dl className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <div>
            <dt className="text-xs uppercase tracking-[0.14em] text-[var(--text-muted)]">Path</dt>
            <dd className="mt-2 break-all font-mono text-sm">{projectPath}</dd>
          </div>
          <div>
            <dt className="text-xs uppercase tracking-[0.14em] text-[var(--text-muted)]">Branch</dt>
            <dd className="mt-2 text-sm">{projectBranch}</dd>
          </div>
          <div>
            <dt className="text-xs uppercase tracking-[0.14em] text-[var(--text-muted)]">Dirty state</dt>
            <dd className="mt-2 text-sm">{projectDirty ? 'Uncommitted changes detected' : 'No changes detected'}</dd>
          </div>
          <div>
            <dt className="text-xs uppercase tracking-[0.14em] text-[var(--text-muted)]">MACC version</dt>
            <dd className="mt-2 text-sm">{maccVersion}</dd>
          </div>
        </dl>
      </section>

      <section className="grid gap-6 xl:grid-cols-[1.25fr_0.75fr]">
        <div className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-5 shadow-[var(--shadow-soft)]">
          <div className="mb-4 flex items-center justify-between">
            <h2 className="text-xl font-semibold">Coordinator summary</h2>
            <StatusBadge status={currentStatusLabel} tone={currentStatusTone} />
          </div>
          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
            <KpiCard title="Total" value={status?.total ?? '...'} />
            <KpiCard title="Todo" value={status?.todo ?? '...'} />
            <KpiCard title="Active" value={status?.active ?? '...'} />
            <KpiCard title="Blocked" value={status?.blocked ?? '...'} />
            <KpiCard title="Merged" value={status?.merged ?? '...'} />
            <KpiCard title="Paused" value={status?.paused ? 1 : 0} />
          </div>

          <div className="mt-5 grid gap-4 md:grid-cols-2">
            <article className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4">
              <p className="text-xs uppercase tracking-[0.14em] text-[var(--text-muted)]">Pause reason</p>
              <p className="mt-2 text-sm">{status?.pause_reason ?? 'No pause reason reported.'}</p>
            </article>
            <article className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4">
              <p className="text-xs uppercase tracking-[0.14em] text-[var(--text-muted)]">Failure report</p>
              <p className="mt-2 text-sm">{summary ?? 'No failure report available.'}</p>
            </article>
          </div>

          {loadError ? (
            <div className="mt-4 rounded-xl border border-[var(--error)]/30 bg-[var(--error)]/10 p-4 text-sm">
              <p className="font-semibold">Unable to load coordinator status</p>
              <p className="mt-1">{loadError}</p>
            </div>
          ) : null}
        </div>

        <aside className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-secondary)] p-5 shadow-[var(--shadow-soft)]">
          <h2 className="text-xl font-semibold">Coordinator controls</h2>
          <p className="mt-2 text-sm text-[var(--text-secondary)]">
            Run and stop are wired to the live API and refresh the dashboard after completion.
          </p>
          <div className="mt-5 space-y-3">
            {ACTIONS.map((config) => {
              const isPending = pendingAction === config.action;
              return (
                <article
                  className="rounded-xl border border-[var(--border)] bg-[var(--bg-card)] p-4"
                  key={config.action}
                >
                  <p className="font-medium">{config.label}</p>
                  <p className="mt-1 text-sm text-[var(--text-secondary)]">{config.description}</p>
                  <Button
                    className={`mt-3 w-full ${actionClassName(config.emphasis)}`}
                    disabled={isBusy || isLoadingStatus || isLoadingAux}
                    onClick={() => {
                      void handleAction(config.action);
                    }}
                    type="button"
                  >
                    {isPending ? 'Working...' : config.label}
                  </Button>
                </article>
              );
            })}
          </div>
        </aside>
      </section>

      <section className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-5 shadow-[var(--shadow-soft)]">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-xl font-semibold">Worktrees summary</h2>
          <StatusBadge status={`${worktreeMetrics.total} total`} tone="todo" />
        </div>
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <KpiCard title="Total" value={worktreeMetrics.total} />
          <KpiCard title="Active" value={worktreeMetrics.active} />
          <KpiCard title="Idle" value={worktreeMetrics.idle} />
          <KpiCard title="Stale" value={worktreeMetrics.stale} />
        </div>
        <div className="mt-5 grid gap-3 md:grid-cols-2">
          {worktrees.slice(0, 6).map((worktree) => {
            const state = (worktree.status ?? 'unknown').toLowerCase();
            const tone: StatusTone =
              state === 'clean'
                ? 'merged'
                : state === 'dirty'
                  ? 'blocked'
                  : state === 'prunable'
                    ? 'failed'
                    : state === 'locked'
                      ? 'active'
                      : 'todo';
            return (
              <article className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-3" key={worktree.id}>
                <div className="flex items-center justify-between gap-3">
                  <p className="truncate font-medium">{worktree.id}</p>
                  <StatusBadge status={state} tone={tone} />
                </div>
                <p className="mt-1 truncate text-xs text-[var(--text-secondary)]">{worktree.path}</p>
              </article>
            );
          })}
          {worktrees.length === 0 ? (
            <p className="text-sm text-[var(--text-secondary)]">No worktrees reported yet.</p>
          ) : null}
        </div>
      </section>

      <section className="grid gap-6 xl:grid-cols-2">
        <article className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-5 shadow-[var(--shadow-soft)]">
          <div className="mb-4 flex items-center justify-between">
            <h2 className="text-xl font-semibold">Recent activity</h2>
            <StatusBadge status={`${recentEvents.length} events`} tone="active" />
          </div>
          <div className="space-y-3">
            {recentEvents.map((entry) => (
              <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-3" key={`${entry.payload.event_id}-${entry.receivedAt}`}>
                <div className="flex items-center justify-between gap-3">
                  <p className="text-sm font-medium">{entry.payload.type}</p>
                  <StatusBadge status={String(entry.payload.status)} tone="todo" />
                </div>
                <p className="mt-2 text-sm text-[var(--text-secondary)]">{summarizeEvent(entry.payload)}</p>
                <p className="mt-2 text-xs text-[var(--text-muted)]">{formatTimestamp(entry.payload.ts)}</p>
              </div>
            ))}
            {recentEvents.length === 0 ? (
              <p className="text-sm text-[var(--text-secondary)]">Waiting for coordinator events from `/api/v1/events`.</p>
            ) : null}
          </div>
        </article>

        <article className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-5 shadow-[var(--shadow-soft)]">
          <div className="mb-4 flex items-center justify-between">
            <h2 className="text-xl font-semibold">Alerts</h2>
            <StatusBadge status={`${alerts.length} open`} tone={alerts.length > 0 ? 'blocked' : 'merged'} />
          </div>
          <div className="space-y-3">
            {alerts.map((alert) => (
              <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-3" key={alert.id}>
                <div className="flex items-center justify-between gap-3">
                  <p className="text-sm font-medium">{alert.title}</p>
                  <StatusBadge status={alert.tone.toUpperCase()} tone={alert.tone} />
                </div>
                <p className="mt-2 text-sm text-[var(--text-secondary)]">{alert.detail}</p>
              </div>
            ))}
            {alerts.length === 0 ? (
              <p className="text-sm text-[var(--text-secondary)]">No active alerts from status or doctor checks.</p>
            ) : null}
          </div>
        </article>
      </section>
    </section>
  );
};

export default Dashboard;
