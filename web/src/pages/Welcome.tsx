import React, { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { getHealth, getStatus, getSnapshot } from '../api/client';
import type { ApiHealthResponse, ApiCoordinatorStatus, ApiRuntimeSnapshot } from '../api/models';
import { CheckIcon, XIcon, ArrowRightIcon } from '../components/icons';
import { cn } from '../components/styles';
import { Icons } from '../components/NavIcons';

interface SetupCard {
  title: string;
  description: string;
  href: string;
  icon: React.ComponentType;
}

const SETUP_CARDS: SetupCard[] = [
  {
    title: 'Configure tools',
    description: 'Detect assistants, generate config, install selected skills and MCP templates.',
    href: '/config/tools',
    icon: Icons.Wrench,
  },
  {
    title: 'Run one task',
    description: 'Select a PRD task, prepare one worktree, and run with supervision.',
    href: '/prd',
    icon: Icons.Brain,
  },
  {
    title: 'Run a batch',
    description: 'Validate the PRD, configure parallelism, and run multiple tasks from the dashboard.',
    href: '/dashboard',
    icon: Icons.Settings,
  },
  {
    title: 'Inspect project',
    description: 'Browse status, backups, diagnostics, config, and logs without making changes.',
    href: '/ops/console',
    icon: Icons.Activity,
  },
];

interface ReadinessItem {
  label: string;
  done: boolean;
  detail?: string;
  action?: { label: string; href: string };
}

function computeReadiness(
  health: ApiHealthResponse | null,
  status: ApiCoordinatorStatus | null,
  snapshot: ApiRuntimeSnapshot | null,
): ReadinessItem[] {
  const hasConfig = !!health?.project_root;
  const hasTask =
    snapshot != null &&
    (snapshot.queue.todo + snapshot.queue.ready + snapshot.queue.in_progress) > 0;
  const coordinatorRunning = status != null && status.active > 0;
  const hasTool = snapshot?.workers != null && snapshot.workers.length > 0;

  return [
    {
      label: 'Project initialized',
      done: hasConfig,
      detail: health?.project_root
        ? health.project_root.split('/').slice(-2).join('/')
        : undefined,
      action: hasConfig ? undefined : { label: 'Initialize', href: '/init' },
    },
    {
      label: 'Tool adapter configured',
      done: hasTool || (snapshot?.workers != null),
      action: { label: 'Configure', href: '/config/tools' },
    },
    {
      label: 'Config applied',
      done: hasConfig,
      action: hasConfig ? undefined : { label: 'Apply config', href: '/init' },
    },
    {
      label: 'PRD task available',
      done: hasTask,
      detail: hasTask
        ? `${(snapshot?.queue.todo ?? 0) + (snapshot?.queue.ready ?? 0)} ready`
        : snapshot != null
          ? 'no tasks found'
          : undefined,
      action: { label: 'Open PRD', href: '/prd' },
    },
    {
      label: 'Coordinator running',
      done: coordinatorRunning,
      detail: coordinatorRunning ? `${status?.active} active` : undefined,
      action: coordinatorRunning ? undefined : { label: 'Start', href: '/ops/live' },
    },
  ];
}

const Welcome: React.FC = () => {
  const [health, setHealth] = useState<ApiHealthResponse | null>(null);
  const [status, setStatus] = useState<ApiCoordinatorStatus | null>(null);
  const [snapshot, setSnapshot] = useState<ApiRuntimeSnapshot | null>(null);

  useEffect(() => {
    getHealth().then(setHealth).catch(() => null);
    getStatus().then(setStatus).catch(() => null);
    getSnapshot().then(setSnapshot).catch(() => null);
  }, []);

  const isRunning = status ? !status.paused && status.active > 0 : false;
  const isPaused = status?.paused ?? false;

  const coordinatorLabel = status
    ? isPaused
      ? 'Paused'
      : status.active > 0
        ? 'Running'
        : 'Idle'
    : 'Unknown';

  const readiness = computeReadiness(health, status, snapshot);
  const blockingCount = readiness.filter((r) => !r.done).length;

  const shortPath = health?.project_root
    ? health.project_root.split('/').slice(-2).join('/')
    : null;

  return (
    <div className="flex max-w-2xl flex-col gap-5">
      {/* Status strip */}
      <div className="flex flex-wrap items-center gap-3">
        <div className="flex items-center gap-2">
          <span
            className="h-2 w-2 shrink-0 rounded-full"
            style={{
              backgroundColor: isRunning
                ? 'var(--success)'
                : isPaused
                  ? 'var(--warning)'
                  : 'var(--border)',
            }}
          />
          <span className="text-sm font-medium text-[var(--text-primary)]">
            {coordinatorLabel}
          </span>
        </div>

        {shortPath && (
          <>
            <span className="text-[var(--border)]" aria-hidden>·</span>
            <span
              className="max-w-[260px] truncate font-mono text-[13px] text-[var(--text-muted)]"
              title={health?.project_root ?? undefined}
            >
              {shortPath}
            </span>
          </>
        )}

        {status && status.total > 0 && (
          <>
            <span className="text-[var(--border)]" aria-hidden>·</span>
            <span className="text-xs text-[var(--text-muted)]">
              {status.total} task{status.total !== 1 ? 's' : ''}
              {status.active > 0 && `, ${status.active} active`}
            </span>
          </>
        )}

        <Link
          to="/dashboard"
          className="ml-auto inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-semibold text-white transition-[filter] hover:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg-primary)]"
          style={{ backgroundColor: 'var(--accent)' }}
        >
          <Icons.Play />
          Open dashboard
        </Link>
      </div>

      {/* Readiness */}
      <section aria-label="Project readiness">
        <div
          className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
          style={{ boxShadow: 'var(--shadow-soft)' }}
        >
          <ol>
            {readiness.map((item, index) => (
              <li
                key={index}
                className={cn(
                  'flex items-center gap-3 px-4 py-2.5',
                  index > 0 && 'border-t border-[var(--border-subtle)]',
                )}
              >
                {item.done ? (
                  <CheckIcon
                    className="h-3.5 w-3.5 shrink-0"
                    style={{ color: 'var(--success)' }}
                  />
                ) : (
                  <XIcon
                    className="h-3.5 w-3.5 shrink-0"
                    style={{ color: 'var(--error)' }}
                  />
                )}
                <span
                  className={cn(
                    'text-sm',
                    item.done ? 'text-[var(--text-primary)]' : 'text-[var(--text-secondary)]',
                  )}
                >
                  {item.label}
                </span>
                {item.detail && (
                  <span className="ml-1 text-xs text-[var(--text-muted)]">{item.detail}</span>
                )}
                {!item.done && item.action && (
                  <Link
                    to={item.action.href}
                    className="ml-auto rounded text-xs font-medium hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent)]"
                    style={{ color: 'var(--accent)' }}
                  >
                    {item.action.label}
                  </Link>
                )}
              </li>
            ))}
          </ol>

          <div className="border-t border-[var(--border-subtle)] px-4 py-2.5 text-xs">
            {blockingCount === 0 ? (
              <span className="font-medium" style={{ color: 'var(--success)' }}>
                Ready to dispatch tasks.
              </span>
            ) : (
              <span className="text-[var(--text-muted)]">
                {blockingCount} step{blockingCount !== 1 ? 's' : ''} remaining —{' '}
                <Link
                  to="/ops/diagnostics"
                  className="hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent)] rounded"
                  style={{ color: 'var(--accent)' }}
                >
                  run diagnostics
                </Link>
              </span>
            )}
          </div>
        </div>
      </section>

      {/* Navigation */}
      <nav aria-label="Quick navigation">
        <ul
          className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
          style={{ boxShadow: 'var(--shadow-soft)' }}
        >
          {SETUP_CARDS.map((card, index) => {
            const Icon = card.icon;
            return (
              <li key={card.href} className={cn(index > 0 && 'border-t border-[var(--border-subtle)]')}>
                <Link
                  to={card.href}
                  className="flex items-center gap-4 px-4 py-3.5 transition-colors hover:bg-[var(--bg-elevated)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--accent)]"
                >
                  <div
                    className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-[var(--border)] bg-[var(--bg-secondary)]"
                    style={{ color: 'var(--accent)' }}
                  >
                    <Icon />
                  </div>
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium text-[var(--text-primary)]">{card.title}</p>
                    <p className="mt-0.5 text-xs text-[var(--text-secondary)]">{card.description}</p>
                  </div>
                  <ArrowRightIcon
                    className="h-4 w-4 shrink-0 text-[var(--text-muted)]"
                  />
                </Link>
              </li>
            );
          })}
        </ul>
      </nav>

      {/* Secondary links */}
      <div className="flex items-center gap-3 text-sm text-[var(--text-muted)]">
        <Link
          to="/init"
          className="hover:text-[var(--text-secondary)] hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent)] rounded"
        >
          Guided setup
        </Link>
        <span aria-hidden>·</span>
        <Link
          to="/ops/diagnostics"
          className="hover:text-[var(--text-secondary)] hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent)] rounded"
        >
          Run diagnostics
        </Link>
      </div>
    </div>
  );
};

export default Welcome;
