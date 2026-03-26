import React from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { buildUrl, getStatus } from '../api/client';
import type { ApiCoordinatorStatus } from '../api/models';
import { Button, StatusBadge } from '../components';
import {
  ActivityIcon,
  ArrowRightIcon,
  BranchIcon,
  CheckCircleIcon,
  DownloadIcon,
  InfoIcon,
  LayoutGridIcon,
  SparklesIcon,
} from '../components/icons';
import { cn, interactiveSurfaceClassName, surfaceClassName } from '../components/styles';

interface OnboardingCard {
  title: string;
  description: string;
  href: string;
  icon: React.ComponentType<{ className?: string }>;
  detail: string;
}

const CARDS: OnboardingCard[] = [
  {
    title: 'Detect & Install Adapters',
    description: 'Inspect available adapters, verify health, and install the ones you need.',
    href: '/config/tools',
    icon: DownloadIcon,
    detail: 'Step 1',
  },
  {
    title: 'Configure Project',
    description: 'Walk through the project wizard and set up the initial MACC configuration.',
    href: '/init',
    icon: LayoutGridIcon,
    detail: 'Step 2',
  },
  {
    title: 'Import Skills',
    description: 'Select skills, agents, and MCP integrations that should be available in the workspace.',
    href: '/config/skills',
    icon: BranchIcon,
    detail: 'Step 3',
  },
];

function hasCoordinatorContent(status: ApiCoordinatorStatus | null): boolean {
  if (!status) {
    return false;
  }

  return (
    status.total > 0 ||
    status.todo > 0 ||
    status.active > 0 ||
    status.blocked > 0 ||
    status.merged > 0 ||
    status.paused ||
    Boolean(status.latest_error) ||
    Boolean(status.failure_report)
  );
}

function safeString(value: unknown): string | null {
  return typeof value === 'string' && value.trim().length > 0 ? value.trim() : null;
}

function extractVersionBadge(payload: unknown): string | null {
  if (typeof payload !== 'object' || payload === null || Array.isArray(payload)) {
    return null;
  }

  const data = payload as Record<string, unknown>;
  const current =
    safeString(data.currentVersion) ??
    safeString(data.current_version) ??
    safeString(data.version);
  const latest =
    safeString(data.latestVersion) ??
    safeString(data.latest_version) ??
    safeString(data.availableVersion) ??
    safeString(data.available_version);
  const updateAvailable =
    typeof data.updateAvailable === 'boolean'
      ? data.updateAvailable
      : typeof data.update_available === 'boolean'
        ? data.update_available
        : undefined;

  if (updateAvailable === true) {
    return latest ? `New version available ${latest}` : 'New version available';
  }

  if (current && latest && current !== latest) {
    return `New version available ${latest}`;
  }

  return null;
}

const Welcome: React.FC = () => {
  const navigate = useNavigate();
  const [isLoading, setIsLoading] = React.useState(true);
  const [status, setStatus] = React.useState<ApiCoordinatorStatus | null>(null);
  const [versionBadge, setVersionBadge] = React.useState<string | null>(null);

  React.useEffect(() => {
    const abortController = new AbortController();
    let isActive = true;

    const load = async (): Promise<void> => {
      try {
        const [statusResult, healthResponse] = await Promise.allSettled([
          getStatus({ signal: abortController.signal }),
          fetch(buildUrl('/health'), { signal: abortController.signal }),
        ]);

        if (!isActive) {
          return;
        }

        if (statusResult.status === 'fulfilled') {
          const nextStatus = statusResult.value;
          setStatus(nextStatus);
          if (hasCoordinatorContent(nextStatus)) {
            navigate('/dashboard', { replace: true });
          }
        }

        if (healthResponse.status === 'fulfilled' && healthResponse.value.ok) {
          const payload = (await healthResponse.value.json().catch(() => null)) as unknown;
          setVersionBadge(extractVersionBadge(payload));
        }
      } finally {
        if (isActive) {
          setIsLoading(false);
        }
      }
    };

    void load();

    return () => {
      isActive = false;
      abortController.abort();
    };
  }, [navigate]);

  const initialized = hasCoordinatorContent(status);

  const handleQuickStart = React.useCallback(() => {
    navigate(initialized ? '/dashboard' : '/init');
  }, [initialized, navigate]);

  return (
    <div className="relative min-h-full overflow-hidden">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_top_left,_rgba(59,130,246,0.18),_transparent_30%),radial-gradient(circle_at_bottom_right,_rgba(34,197,94,0.12),_transparent_34%),linear-gradient(180deg,_rgba(15,23,42,0.94),_rgba(10,10,10,1))]" />
      <div className="pointer-events-none absolute inset-x-0 top-0 h-48 bg-[linear-gradient(90deg,_transparent,_rgba(255,255,255,0.06),_transparent)] opacity-40" />

      <div className="relative mx-auto flex min-h-full w-full max-w-7xl flex-col gap-8 px-4 py-8 sm:px-6 lg:px-10">
        <header className={cn(surfaceClassName, 'relative overflow-hidden border-white/8 bg-[rgba(22,22,22,0.9)] p-6 sm:p-8')}>
          <div className="absolute inset-0 bg-[radial-gradient(circle_at_top_right,_rgba(59,130,246,0.16),_transparent_28%)]" />
          <div className="relative grid gap-8 lg:grid-cols-[1.35fr_0.85fr] lg:items-start">
            <section className="space-y-6">
              <div className="flex flex-wrap items-center gap-3">
                <StatusBadge status={initialized ? 'Ready for work' : 'First run'} tone={initialized ? 'merged' : 'todo'} />
                {versionBadge ? <StatusBadge status={versionBadge} tone="blocked" /> : null}
                {isLoading ? (
                  <span className="rounded-full border border-white/10 bg-white/5 px-3 py-1 text-xs font-medium text-[var(--text-secondary)]">
                    Checking project status...
                  </span>
                ) : null}
              </div>

              <div className="max-w-3xl space-y-4">
                <p className="inline-flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.3em] text-[var(--text-muted)]">
                  <SparklesIcon className="h-4 w-4 text-[var(--accent)]" />
                  Welcome to MACC
                </p>
                <h1 className="text-4xl font-semibold tracking-tight text-[var(--text-primary)] sm:text-5xl lg:text-6xl">
                  Set up the workspace in three guided steps.
                </h1>
                <p className="max-w-2xl text-base leading-7 text-[var(--text-secondary)] sm:text-lg">
                  Start with adapter detection, move into project initialization, and finish by importing the skills
                  that define your local workflow.
                </p>
              </div>

              <div className="flex flex-wrap gap-3">
                <Button
                  className="h-12 rounded-full border-transparent bg-[var(--accent)] px-5 text-base font-semibold text-white hover:brightness-110"
                  onClick={handleQuickStart}
                  type="button"
                >
                  Quick Start
                </Button>
                <Button asChild className="h-12 rounded-full px-5 text-base font-semibold">
                  <Link to="/init">Open init wizard</Link>
                </Button>
              </div>

              <p className="flex items-start gap-2 text-sm text-[var(--text-muted)]">
                <InfoIcon className="mt-0.5 h-4 w-4 shrink-0" />
                Quick Start opens the initialization flow, then returns you to the dashboard once the project is ready.
              </p>
            </section>

            <aside className={cn(surfaceClassName, 'relative overflow-hidden border-white/8 bg-black/20 p-5')}>
              <div className="absolute inset-0 bg-[linear-gradient(160deg,_rgba(59,130,246,0.08),_transparent_40%)]" />
              <div className="relative space-y-5">
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <p className="text-xs font-semibold uppercase tracking-[0.3em] text-[var(--text-muted)]">
                      Guided onboarding
                    </p>
                    <h2 className="mt-2 text-xl font-semibold text-[var(--text-primary)]">What happens next</h2>
                  </div>
                  <StatusBadge status={initialized ? 'Live' : 'Waiting'} tone={initialized ? 'active' : 'todo'} />
                </div>

                <div className="space-y-3">
                  {[
                    'Detect the tools your repository can use.',
                    'Write the initial project configuration.',
                    'Choose the skills and agents to enable.',
                  ].map((item) => (
                    <div key={item} className="flex items-start gap-3 rounded-2xl border border-white/6 bg-white/4 p-3">
                      <CheckCircleIcon className="mt-0.5 h-4 w-4 shrink-0 text-[var(--success)]" />
                      <span className="text-sm leading-6 text-[var(--text-secondary)]">{item}</span>
                    </div>
                  ))}
                </div>

                <div className="rounded-2xl border border-white/8 bg-black/25 p-4 text-sm text-[var(--text-secondary)]">
                  <div className="flex items-center gap-2 text-[var(--text-primary)]">
                    <ActivityIcon className="h-4 w-4 text-[var(--accent)]" />
                    Project status
                  </div>
                  <p className="mt-2 leading-6">
                    {initialized
                      ? 'A coordinator snapshot is available, so the workspace is ready for the dashboard.'
                      : 'No active coordinator snapshot was detected yet. Start with Quick Start or open the init wizard.'}
                  </p>
                </div>
              </div>
            </aside>
          </div>
        </header>

        <section className="grid gap-4 md:grid-cols-3">
          {CARDS.map((card) => {
            const Icon = card.icon;
            return (
              <Link
                key={card.href}
                className={cn(
                  surfaceClassName,
                  interactiveSurfaceClassName,
                  'group flex h-full flex-col justify-between gap-6 border-white/8 bg-[rgba(22,22,22,0.85)] p-5 transition-transform duration-200 hover:-translate-y-1',
                )}
                to={card.href}
              >
                <div className="flex items-start justify-between gap-4">
                  <div className="space-y-3">
                    <StatusBadge status={card.detail} tone="todo" />
                    <h2 className="text-xl font-semibold tracking-tight text-[var(--text-primary)]">{card.title}</h2>
                    <p className="max-w-sm text-sm leading-6 text-[var(--text-secondary)]">{card.description}</p>
                  </div>
                  <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-2xl border border-white/10 bg-white/5 text-[var(--accent)]">
                    <Icon className="h-5 w-5" />
                  </div>
                </div>

                <div className="flex items-center justify-between text-sm font-medium text-[var(--text-primary)]">
                  <span>Open step</span>
                  <ArrowRightIcon className="h-4 w-4 text-[var(--text-muted)] transition-transform duration-200 group-hover:translate-x-1 group-hover:text-[var(--text-primary)]" />
                </div>
              </Link>
            );
          })}
        </section>
      </div>
    </div>
  );
};

export default Welcome;
