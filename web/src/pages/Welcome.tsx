import React, { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { getHealth, getStatus, getSnapshot } from '../api/client';
import type { ApiHealthResponse, ApiCoordinatorStatus, ApiRuntimeSnapshot } from '../api/models';
import { StatusBadge } from '../components/StatusBadge';
import { surfaceClassName, interactiveSurfaceClassName, cn } from '../components/styles';
import { Icons } from '../components/NavIcons';

interface SetupCard {
  title: string;
  description: string;
  href: string;
  icon: React.ComponentType;
  cta: string;
}

const SETUP_CARDS: SetupCard[] = [
  {
    title: 'Configure Tools',
    description: 'Detect assistants, generate config, install selected skills/MCP templates.',
    href: '/config/tools',
    icon: Icons.Wrench,
    cta: 'Configure Tools',
  },
  {
    title: 'Run One Task',
    description: 'Create or select a PRD task, prepare one worktree, run with supervision.',
    href: '/prd',
    icon: Icons.Brain,
    cta: 'Run Task',
  },
  {
    title: 'Run a Batch',
    description: 'Prepare coordinator settings, validate PRD, run multiple tasks with dashboard.',
    href: '/dashboard',
    icon: Icons.Settings,
    cta: 'Run Batch',
  },
  {
    title: 'Inspect Project',
    description: 'Open status, backups, diagnostics, config, and logs without writing.',
    href: '/ops/console',
    icon: Icons.Activity,
    cta: 'Inspect Project',
  },
];

// Readiness step computed from snapshot data (spec §13.2)
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
  const hasTool =
    snapshot?.workers != null && snapshot.workers.length > 0;

  return [
    {
      label: 'Project initialized',
      done: hasConfig,
      detail: health?.project_root
        ? health.project_root.split('/').slice(-2).join('/')
        : undefined,
      action: hasConfig ? undefined : { label: 'Init', href: '/init' },
    },
    {
      label: 'Tool adapter configured',
      done: hasTool || (snapshot?.workers != null),
      action: { label: 'Configure', href: '/config/tools' },
    },
    {
      label: 'Config applied',
      done: hasConfig,
      action: hasConfig ? undefined : { label: 'Apply', href: '/init' },
    },
    {
      label: 'PRD / task available',
      done: hasTask,
      detail: hasTask
        ? `${(snapshot?.queue.todo ?? 0) + (snapshot?.queue.ready ?? 0)} ready`
        : snapshot != null ? 'no tasks found' : undefined,
      action: { label: 'PRD', href: '/prd' },
    },
    {
      label: 'Coordinator running',
      done: coordinatorRunning,
      detail: coordinatorRunning ? `${status?.active} active` : undefined,
      action: coordinatorRunning
        ? undefined
        : { label: 'Start', href: '/ops/live' },
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

  const coordinatorTone = status
    ? status.paused
      ? 'paused'
      : status.active > 0
        ? 'active'
        : 'todo'
    : 'todo';

  const coordinatorLabel = status
    ? status.paused
      ? 'Paused'
      : status.active > 0
        ? 'Running'
        : 'Idle'
    : 'Unknown';

  const readiness = computeReadiness(health, status, snapshot);
  const blockingCount = readiness.filter((r) => !r.done).length;

  return (
    <div className="flex flex-col gap-6 max-w-4xl">
      {/* Header */}
      <header className={cn(surfaceClassName, 'p-6')}>
        <div className="flex items-start justify-between gap-4">
          <div>
            <h1 className="text-3xl font-semibold tracking-tight text-[var(--text-primary)]">
              Welcome to MACC
            </h1>
            <p className="mt-2 text-sm text-[var(--text-secondary)]">
              Multi-Agent Coding Coordinator — orchestrate AI coding tools across your project.
            </p>
            {health?.project_root && (
              <p className="mt-2 font-mono text-xs text-[var(--text-muted)] truncate max-w-[480px]">
                {health.project_root}
              </p>
            )}
          </div>
          {status && (
            <div className="flex flex-col items-end gap-1 shrink-0">
              <span className="text-xs text-[var(--text-muted)]">Coordinator</span>
              <StatusBadge status={coordinatorLabel} tone={coordinatorTone} className="px-2 py-0.5 text-xs" />
              {status.total > 0 && (
                <span className="text-xs text-[var(--text-muted)]">
                  {status.total} task{status.total !== 1 ? 's' : ''} · {status.active} active
                </span>
              )}
            </div>
          )}
        </div>
      </header>

      {/* Readiness ladder (spec §13.2) */}
      <section aria-labelledby="readiness-heading">
        <h2
          id="readiness-heading"
          className="mb-3 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]"
        >
          Readiness
        </h2>
        <div className={cn(surfaceClassName, 'p-4')}>
          <ol className="flex flex-col gap-2">
            {readiness.map((item, i) => (
              <li key={i} className="flex items-center gap-3 text-sm">
                <span className="w-5 text-center" aria-hidden="true">
                  {item.done ? '✅' : '❌'}
                </span>
                <span
                  className={
                    item.done
                      ? 'text-[var(--text-primary)]'
                      : 'text-[var(--text-secondary)]'
                  }
                >
                  {item.label}
                </span>
                {item.detail && (
                  <span className="ml-1 text-xs text-[var(--text-muted)]">{item.detail}</span>
                )}
                {!item.done && item.action && (
                  <Link
                    to={item.action.href}
                    className="ml-auto text-xs text-[var(--accent)] hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent)] rounded"
                  >
                    {item.action.label} →
                  </Link>
                )}
              </li>
            ))}
          </ol>
          {blockingCount === 0 ? (
            <p className="mt-4 text-xs text-green-400 font-medium">
              ✅ Ready to dispatch a task
            </p>
          ) : (
            <p className="mt-4 text-xs text-[var(--text-muted)]">
              {blockingCount} step{blockingCount !== 1 ? 's' : ''} pending —{' '}
              <Link
                to="/ops/diagnostics"
                className="text-[var(--accent)] hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent)] rounded"
              >
                run diagnostics
              </Link>
            </p>
          )}
        </div>
      </section>

      {/* Setup Cards */}
      <section aria-labelledby="setup-heading">
        <h2
          id="setup-heading"
          className="mb-3 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]"
        >
          Get Started
        </h2>
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          {SETUP_CARDS.map((card) => {
            const Icon = card.icon;
            return (
              <Link
                key={card.href}
                to={card.href}
                className={cn(
                  surfaceClassName,
                  interactiveSurfaceClassName,
                  'flex flex-col gap-3 p-5 no-underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]',
                )}
              >
                <div className="flex items-center gap-3">
                  <div className="rounded-lg border border-white/10 bg-white/5 p-2 text-[var(--accent)]">
                    <Icon />
                  </div>
                  <span className="font-medium text-[var(--text-primary)] text-sm">{card.title}</span>
                </div>
                <p className="text-xs text-[var(--text-secondary)] leading-relaxed">{card.description}</p>
                <span className="mt-auto text-xs font-medium text-[var(--accent)]">{card.cta} →</span>
              </Link>
            );
          })}
        </div>
      </section>

      {/* CTAs */}
      <section className="flex items-center gap-3">
        <Link
          to="/dashboard"
          className="inline-flex items-center gap-2 rounded-md bg-[var(--accent)] px-4 py-2 text-sm font-semibold text-white shadow-sm transition-opacity hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]"
        >
          <Icons.Play />
          Open Dashboard
        </Link>
        <Link
          to="/init"
          className="inline-flex items-center gap-2 rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-4 py-2 text-sm font-medium text-[var(--text-secondary)] transition-colors hover:bg-[var(--bg-card)] hover:text-[var(--text-primary)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]"
        >
          <Icons.List />
          Guided Setup
        </Link>
      </section>
    </div>
  );
};

export default Welcome;
