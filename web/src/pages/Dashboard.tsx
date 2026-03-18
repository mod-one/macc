import React from 'react';
import { Button } from '../components/Button';
import { ApiClientError, getStatus, postCoordinatorAction } from '../api/client';
import type {
  ApiCoordinatorAction,
  ApiCoordinatorCommandResult,
  ApiCoordinatorStatus,
  ApiFailureReport,
} from '../api/models';

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

interface StatCardProps {
  label: string;
  value: number | string;
  tone?: 'default' | 'accent' | 'warning';
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

function formatApiError(error: unknown): string {
  if (error instanceof ApiClientError) {
    return `${error.envelope.error.code}: ${error.envelope.error.message}`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return 'Unexpected coordinator error.';
}

function formatResultSummary(
  action: ApiCoordinatorAction,
  result: ApiCoordinatorCommandResult,
): string {
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

function failureSummary(report: ApiFailureReport | null): string | null {
  if (!report) {
    return null;
  }
  return `${report.source}: ${report.message}`;
}

function actionClassName(emphasis: CoordinatorActionConfig['emphasis']): string {
  if (emphasis === 'primary') {
    return 'bg-slate-900 text-white hover:bg-slate-800';
  }
  if (emphasis === 'danger') {
    return 'bg-rose-600 text-white hover:bg-rose-500';
  }
  return 'bg-white text-slate-900 border border-slate-300 hover:bg-slate-100';
}

function noticeClassName(tone: NoticeTone): string {
  if (tone === 'success') {
    return 'border-emerald-300 bg-emerald-50 text-emerald-900';
  }
  return 'border-rose-300 bg-rose-50 text-rose-900';
}

const StatCard: React.FC<StatCardProps> = ({ label, value, tone = 'default' }) => {
  const toneClassName =
    tone === 'accent'
      ? 'border-sky-200 bg-sky-50'
      : tone === 'warning'
        ? 'border-amber-200 bg-amber-50'
        : 'border-slate-200 bg-white';

  return (
    <article className={`rounded-2xl border p-4 shadow-sm ${toneClassName}`}>
      <p className="text-sm font-medium uppercase tracking-[0.16em] text-slate-500">{label}</p>
      <p className="mt-3 text-3xl font-semibold text-slate-900">{value}</p>
    </article>
  );
};

const Dashboard: React.FC = () => {
  const [status, setStatus] = React.useState<ApiCoordinatorStatus | null>(null);
  const [loadError, setLoadError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<NoticeState | null>(null);
  const [isLoadingStatus, setIsLoadingStatus] = React.useState(true);
  const [pendingAction, setPendingAction] = React.useState<ApiCoordinatorAction | null>(null);

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

  const loadCoordinatorStatus = React.useCallback(
    async (signal?: AbortSignal): Promise<ApiCoordinatorStatus | null> => {
      setIsLoadingStatus(true);
      try {
        const nextStatus = await getStatus({ signal });
        setStatus(nextStatus);
        setLoadError(null);
        return nextStatus;
      } catch (error) {
        if (error instanceof DOMException && error.name === 'AbortError') {
          return null;
        }
        const message = formatApiError(error);
        setLoadError(message);
        showNotice('error', message);
        return null;
      } finally {
        setIsLoadingStatus(false);
      }
    },
    [showNotice],
  );

  React.useEffect(() => {
    const abortController = new AbortController();
    void loadCoordinatorStatus(abortController.signal);

    return () => {
      abortController.abort();
    };
  }, [loadCoordinatorStatus]);

  const handleAction = React.useCallback(
    async (action: ApiCoordinatorAction): Promise<void> => {
      setPendingAction(action);

      try {
        const result = await postCoordinatorAction(action);

        if (result.status) {
          setStatus(result.status);
          setLoadError(null);
        } else {
          await loadCoordinatorStatus();
        }

        showNotice('success', formatResultSummary(action, result));
      } catch (error) {
        const message = formatApiError(error);
        setLoadError(message);
        showNotice('error', message);
      } finally {
        setPendingAction(null);
      }
    },
    [loadCoordinatorStatus, showNotice],
  );

  const summary = failureSummary(status?.failure_report ?? null);
  const currentStatusLabel = statusLabel(status);
  const isBusy = pendingAction !== null;

  return (
    <section className="relative mx-auto flex w-full max-w-6xl flex-col gap-6 text-slate-700">
      {notice ? (
        <div className="pointer-events-none fixed right-4 top-4 z-20 w-[min(28rem,calc(100vw-2rem))]">
          <div className={`rounded-2xl border px-4 py-3 shadow-lg ${noticeClassName(notice.tone)}`}>
            <p className="text-sm font-semibold">
              {notice.tone === 'success' ? 'Coordinator updated' : 'Action failed'}
            </p>
            <p className="mt-1 text-sm leading-6">{notice.message}</p>
          </div>
        </div>
      ) : null}

      <header className="rounded-[2rem] border border-slate-200 bg-[radial-gradient(circle_at_top_left,_rgba(125,211,252,0.25),_transparent_35%),linear-gradient(135deg,#ffffff,#f8fafc)] p-6 shadow-sm">
        <div className="flex flex-col gap-6 lg:flex-row lg:items-end lg:justify-between">
          <div className="max-w-2xl">
            <p className="text-sm font-semibold uppercase tracking-[0.2em] text-sky-700">
              Daily orchestration
            </p>
            <h1 className="mb-3 mt-2 text-5xl font-semibold tracking-tight text-slate-950">
              Dashboard
            </h1>
            <p className="max-w-xl text-base leading-7 text-slate-600">
              Monitor coordinator health, inspect queue pressure, and trigger the next control
              action without leaving the web UI.
            </p>
          </div>

          <div className="grid gap-3 rounded-2xl border border-slate-200 bg-white/90 p-4 text-sm text-slate-600 shadow-sm sm:grid-cols-2">
            <div>
              <p className="font-medium uppercase tracking-[0.16em] text-slate-500">Coordinator</p>
              <p className="mt-2 text-2xl font-semibold text-slate-950">{currentStatusLabel}</p>
            </div>
            <div>
              <p className="font-medium uppercase tracking-[0.16em] text-slate-500">Queue health</p>
              <p className="mt-2 text-2xl font-semibold text-slate-950">
                {status ? `${status.todo} remaining` : 'Loading'}
              </p>
            </div>
          </div>
        </div>
      </header>

      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-5">
        <StatCard label="Total tasks" value={status?.total ?? '...'} />
        <StatCard label="To do" value={status?.todo ?? '...'} tone="accent" />
        <StatCard label="Active" value={status?.active ?? '...'} tone="accent" />
        <StatCard label="Blocked" value={status?.blocked ?? '...'} tone="warning" />
        <StatCard label="Merged" value={status?.merged ?? '...'} />
      </div>

      <div className="grid gap-6 xl:grid-cols-[1.2fr_0.8fr]">
        <section className="rounded-[2rem] border border-slate-200 bg-white p-6 shadow-sm">
          <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <h2 className="text-2xl font-semibold text-slate-950">Coordinator status</h2>
              <p className="text-sm leading-6 text-slate-500">
                Latest API snapshot for queue state, pause context, and runtime issues.
              </p>
            </div>
            <Button
              className="bg-white text-slate-900 border border-slate-300 hover:bg-slate-100"
              disabled={isLoadingStatus || isBusy}
              onClick={() => {
                void loadCoordinatorStatus();
              }}
              type="button"
            >
              {isLoadingStatus ? 'Refreshing...' : 'Refresh status'}
            </Button>
          </div>

          {loadError ? (
            <div className="mt-6 rounded-2xl border border-rose-200 bg-rose-50 p-4 text-sm text-rose-900">
              <p className="font-semibold">Unable to load coordinator status</p>
              <p className="mt-1 leading-6">{loadError}</p>
            </div>
          ) : null}

          <dl className="mt-6 grid gap-4 md:grid-cols-2">
            <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
              <dt className="text-sm font-medium uppercase tracking-[0.16em] text-slate-500">
                Pause state
              </dt>
              <dd className="mt-3 text-lg font-semibold text-slate-950">
                {status?.paused ? 'Paused' : 'Active'}
              </dd>
              <p className="mt-2 text-sm leading-6 text-slate-600">
                {status?.pause_reason ?? 'No pause reason reported.'}
              </p>
            </div>

            <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
              <dt className="text-sm font-medium uppercase tracking-[0.16em] text-slate-500">
                Effective parallelism
              </dt>
              <dd className="mt-3 text-lg font-semibold text-slate-950">
                {status?.effective_max_parallel ?? 'Default'}
              </dd>
              <p className="mt-2 text-sm leading-6 text-slate-600">
                {status?.throttled_tools?.length
                  ? `${status.throttled_tools.length} tools currently throttled.`
                  : 'No active rate-limit throttles reported.'}
              </p>
            </div>
          </dl>

          <div className="mt-6 grid gap-4 lg:grid-cols-2">
            <article className="rounded-2xl border border-slate-200 p-4">
              <p className="text-sm font-medium uppercase tracking-[0.16em] text-slate-500">
                Latest error
              </p>
              <p className="mt-3 text-sm leading-6 text-slate-700">
                {status?.latest_error ?? 'No coordinator error reported.'}
              </p>
            </article>

            <article className="rounded-2xl border border-slate-200 p-4">
              <p className="text-sm font-medium uppercase tracking-[0.16em] text-slate-500">
                Failure report
              </p>
              <p className="mt-3 text-sm leading-6 text-slate-700">
                {summary ?? 'No failure report available.'}
              </p>
            </article>
          </div>

          {status?.throttled_tools?.length ? (
            <div className="mt-6 rounded-2xl border border-amber-200 bg-amber-50 p-4">
              <p className="text-sm font-semibold uppercase tracking-[0.16em] text-amber-900">
                Throttled tools
              </p>
              <ul className="mt-3 space-y-2 text-sm text-amber-950">
                {status.throttled_tools.map((tool) => (
                  <li
                    className="rounded-xl border border-amber-200 bg-white/80 px-3 py-2"
                    key={`${tool.tool_id}-${tool.throttled_until}`}
                  >
                    <span className="font-semibold">{tool.tool_id}</span>
                    {` throttled until ${tool.throttled_until} (${tool.consecutive_count} consecutive events)`}
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </section>

        <aside className="rounded-[2rem] border border-slate-200 bg-slate-950 p-6 text-slate-100 shadow-sm">
          <h2 className="text-2xl font-semibold text-white">Coordinator controls</h2>
          <p className="mt-2 text-sm leading-6 text-slate-300">
            Run and stop are wired to the live API. Each action refreshes the status snapshot and
            reports success or failure in a toast.
          </p>

          <div className="mt-6 space-y-4">
            {ACTIONS.map((config) => {
              const isPending = pendingAction === config.action;
              return (
                <article
                  className="rounded-2xl border border-white/10 bg-white/5 p-4 backdrop-blur"
                  key={config.action}
                >
                  <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                    <div>
                      <p className="text-lg font-semibold text-white">{config.label}</p>
                      <p className="mt-1 text-sm leading-6 text-slate-300">{config.description}</p>
                    </div>
                    <Button
                      className={actionClassName(config.emphasis)}
                      disabled={isBusy || isLoadingStatus}
                      onClick={() => {
                        void handleAction(config.action);
                      }}
                      type="button"
                    >
                      {isPending ? 'Working...' : config.label}
                    </Button>
                  </div>
                </article>
              );
            })}
          </div>

          <div className="mt-6 rounded-2xl border border-white/10 bg-white/5 p-4">
            <p className="text-sm font-semibold uppercase tracking-[0.16em] text-slate-300">
              Current session
            </p>
            <p className="mt-3 text-sm leading-6 text-slate-200">
              {isLoadingStatus
                ? 'Loading coordinator snapshot...'
                : status
                  ? `Queue contains ${status.total} tasks with ${status.active} currently active and ${status.blocked} blocked.`
                  : 'No status snapshot available yet.'}
            </p>
          </div>
        </aside>
      </div>
    </section>
  );
};

export default Dashboard;
