import React, { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { getHealth, getStatus } from '../api/client';
import type { ApiHealthResponse, ApiCoordinatorStatus } from '../api/models';
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
    title: 'Detect & Install Adapters',
    description: 'Scan the project for supported AI coding tools and install the MACC adapter for each.',
    href: '/config/tools',
    icon: Icons.Wrench,
    cta: 'Go to Tools',
  },
  {
    title: 'Configure Project',
    description: 'Run the initialization wizard to create .macc/macc.yaml and set project-level defaults.',
    href: '/init',
    icon: Icons.Settings,
    cta: 'Open Wizard',
  },
  {
    title: 'Import Skills',
    description: 'Browse and install agent skills from the catalog to extend coordinator capabilities.',
    href: '/config/skills',
    icon: Icons.Brain,
    cta: 'Open Catalog',
  },
];

const Welcome: React.FC = () => {
  const [health, setHealth] = useState<ApiHealthResponse | null>(null);
  const [status, setStatus] = useState<ApiCoordinatorStatus | null>(null);

  useEffect(() => {
    getHealth().then(setHealth).catch(() => null);
    getStatus().then(setStatus).catch(() => null);
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

      {/* Setup Cards */}
      <section aria-labelledby="setup-heading">
        <h2 id="setup-heading" className="mb-3 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
          Get Started
        </h2>
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
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
          Quick Start
        </Link>
        <Link
          to="/init"
          className="inline-flex items-center gap-2 rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-4 py-2 text-sm font-medium text-[var(--text-secondary)] transition-colors hover:bg-[var(--bg-card)] hover:text-[var(--text-primary)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]"
        >
          <Icons.List />
          Start Guided Tour
        </Link>
      </section>
    </div>
  );
};

export default Welcome;
