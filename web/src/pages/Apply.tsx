import React from 'react';
import { Link, useLocation } from 'react-router-dom';
import { ApiClientError, runApply, runPlan } from '../api/client';
import type { ApiApplyResponse, ApiPlanResponse } from '../api/models';
import { Button, ConfirmDialog, ErrorBanner, LoadingSpinner } from '../components';

type ApplyMode = 'apply-now' | 'dry-run';

type PlanSource = 'preloaded' | 'fresh';

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isPlanSummary(value: unknown): value is ApiPlanResponse['summary'] {
  if (!isRecord(value)) {
    return false;
  }

  return (
    typeof value.totalActions === 'number' &&
    typeof value.filesWrite === 'number' &&
    typeof value.filesMerge === 'number' &&
    typeof value.consentRequired === 'number' &&
    typeof value.backupRequired === 'number' &&
    typeof value.backupPath === 'string'
  );
}

function isApiPlanResponse(value: unknown): value is ApiPlanResponse {
  if (!isRecord(value)) {
    return false;
  }

  return (
    isPlanSummary(value.summary) &&
    Array.isArray(value.files) &&
    Array.isArray(value.diffs) &&
    Array.isArray(value.risks) &&
    Array.isArray(value.consents)
  );
}

function extractPreloadedPlan(state: unknown): ApiPlanResponse | null {
  if (isApiPlanResponse(state)) {
    return state;
  }

  if (!isRecord(state)) {
    return null;
  }

  const candidates: unknown[] = [state.plan, state.planResult, state.planResponse];
  for (const candidate of candidates) {
    if (isApiPlanResponse(candidate)) {
      return candidate;
    }
  }

  return null;
}

function formatError(error: unknown): string {
  if (error instanceof ApiClientError) {
    return `${error.envelope.error.message} (${error.envelope.error.code})`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return 'Unexpected error.';
}

function riskClasses(level: string): string {
  if (level === 'dangerous') {
    return 'border-rose-500/40 bg-rose-500/10 text-rose-300';
  }
  if (level === 'caution') {
    return 'border-amber-500/40 bg-amber-500/10 text-amber-300';
  }
  return 'border-emerald-500/40 bg-emerald-500/10 text-emerald-300';
}

function RiskPill({ level }: { level: string }) {
  return (
    <span className={`inline-flex rounded-full border px-2.5 py-1 text-xs font-semibold uppercase tracking-wide ${riskClasses(level)}`}>
      {level}
    </span>
  );
}

const Apply: React.FC = () => {
  const location = useLocation();

  const [plan, setPlan] = React.useState<ApiPlanResponse | null>(null);
  const [planSource, setPlanSource] = React.useState<PlanSource | null>(null);
  const [loadingPlan, setLoadingPlan] = React.useState(false);
  const [planError, setPlanError] = React.useState<string | null>(null);

  const [mode, setMode] = React.useState<ApplyMode>('apply-now');
  const [confirmOpen, setConfirmOpen] = React.useState(false);
  const [applying, setApplying] = React.useState(false);
  const [applyError, setApplyError] = React.useState<string | null>(null);
  const [applyResult, setApplyResult] = React.useState<ApiApplyResponse | null>(null);

  const loadFreshPlan = React.useCallback(async () => {
    setLoadingPlan(true);
    setPlanError(null);

    try {
      const response = await runPlan({
        scope: 'project',
        allowUserScope: false,
        includeDiff: true,
        explain: true,
      });
      setPlan(response);
      setPlanSource('fresh');
    } catch (error) {
      setPlanError(formatError(error));
    } finally {
      setLoadingPlan(false);
    }
  }, []);

  React.useEffect(() => {
    const preloadedPlan = extractPreloadedPlan(location.state);
    if (preloadedPlan) {
      setPlan(preloadedPlan);
      setPlanSource('preloaded');
      setPlanError(null);
      return;
    }

    void loadFreshPlan();
  }, [loadFreshPlan, location.state]);

  const executeApply = React.useCallback(
    async (dryRun: boolean) => {
      setApplyError(null);
      setApplying(true);

      try {
        const response = await runApply({
          scope: 'project',
          allowUserScope: false,
          dryRun,
          yes: dryRun ? undefined : true,
        });
        setApplyResult(response);
      } catch (error) {
        setApplyError(formatError(error));
      } finally {
        setApplying(false);
      }
    },
    [],
  );

  const filesWritten = React.useMemo(() => {
    if (!applyResult) {
      return 0;
    }

    return applyResult.results.filter((result) => result.success).length;
  }, [applyResult]);

  const backupPaths = React.useMemo(() => {
    const values = new Set<string>();

    if (plan?.summary.backupPath) {
      values.add(plan.summary.backupPath);
    }
    if (applyResult) {
      for (const locationPath of applyResult.backupLocations) {
        values.add(locationPath);
      }
      for (const result of applyResult.results) {
        if (result.backupLocation) {
          values.add(result.backupLocation);
        }
      }
    }

    return Array.from(values);
  }, [applyResult, plan]);

  const handleRunDry = React.useCallback(() => {
    void executeApply(true);
  }, [executeApply]);

  const handleConfirmApply = React.useCallback(() => {
    setConfirmOpen(false);
    void executeApply(false);
  }, [executeApply]);

  return (
    <div className="flex flex-col gap-6 pb-8">
      <header className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6 shadow-sm">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <h1 className="text-3xl font-semibold tracking-tight text-[var(--text-primary)]">Apply</h1>
            <p className="mt-2 max-w-3xl text-sm text-[var(--text-secondary)]">
              Execute planned changes with a single caution-level confirmation gate. Dry run mode previews outcomes without writing files.
            </p>
          </div>
          <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs font-semibold uppercase tracking-wide text-amber-300">
            Caution-level action
          </div>
        </div>
      </header>

      {loadingPlan && (
        <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
          <LoadingSpinner label="Loading plan" />
          <p className="mt-2 text-sm text-[var(--text-secondary)]">Loading plan summary and diff preview...</p>
        </section>
      )}

      {planError && (
        <ErrorBanner
          title="Unable to load plan"
          message={planError}
          onRetry={() => {
            void loadFreshPlan();
          }}
        />
      )}

      {plan && (
        <>
          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
            <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
              <h2 className="text-lg font-semibold text-[var(--text-primary)]">Plan Summary</h2>
              <span className="rounded-md border border-[var(--border)] bg-[var(--bg-card)] px-2.5 py-1 text-xs text-[var(--text-secondary)]">
                Source: {planSource ?? 'unknown'}
              </span>
            </div>

            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              <SummaryCard label="Total Actions" value={plan.summary.totalActions} />
              <SummaryCard label="Files Write" value={plan.summary.filesWrite} />
              <SummaryCard label="Files Merge" value={plan.summary.filesMerge} />
              <SummaryCard label="Consent Required" value={plan.summary.consentRequired} />
              <SummaryCard label="Backup Required" value={plan.summary.backupRequired} />
              <SummaryCard label="Plan Files" value={plan.files.length} />
            </div>

            <div className="mt-4 rounded-xl border border-[var(--border)] bg-[var(--bg-card)] p-3">
              <p className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)]">Backup location</p>
              <p className="mt-1 font-mono text-xs text-[var(--text-primary)] break-all">{plan.summary.backupPath || 'No backup path returned.'}</p>
              <div className="mt-3">
                <Link className="text-sm font-medium text-[var(--accent)] hover:underline" to="/ops/backups">
                  View Backup
                </Link>
              </div>
            </div>
          </section>

          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
            <h2 className="text-lg font-semibold text-[var(--text-primary)]">Planned Files</h2>
            {plan.files.length === 0 ? (
              <p className="mt-3 text-sm text-[var(--text-secondary)]">No file actions in the current plan.</p>
            ) : (
              <div className="mt-3 overflow-x-auto rounded-xl border border-[var(--border)]">
                <table className="w-full min-w-[720px] text-left text-sm">
                  <thead className="bg-[var(--bg-card)]">
                    <tr>
                      <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Path</th>
                      <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Kind</th>
                      <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Risk</th>
                      <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Consent</th>
                      <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Backup</th>
                    </tr>
                  </thead>
                  <tbody>
                    {plan.files.map((file) => (
                      <tr className="border-t border-[var(--border)]" key={`${file.path}:${file.kind}`}>
                        <td className="px-3 py-2 font-mono text-xs text-[var(--text-primary)]">{file.path}</td>
                        <td className="px-3 py-2 text-[var(--text-secondary)]">{file.kind}</td>
                        <td className="px-3 py-2"><RiskPill level={file.riskLevel} /></td>
                        <td className="px-3 py-2 text-[var(--text-secondary)]">{file.consentRequired ? 'Required' : 'No'}</td>
                        <td className="px-3 py-2 text-[var(--text-secondary)]">{file.backupRequired ? 'Required' : 'No'}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>

          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
            <h2 className="text-lg font-semibold text-[var(--text-primary)]">Diff Preview</h2>
            {plan.diffs.length === 0 ? (
              <p className="mt-3 text-sm text-[var(--text-secondary)]">No diff preview returned by the planner.</p>
            ) : (
              <div className="mt-3 space-y-3">
                {plan.diffs.map((diff) => (
                  <article className="rounded-xl border border-[var(--border)] bg-[var(--bg-card)]" key={`${diff.path}:${diff.diffKind}`}>
                    <div className="flex flex-wrap items-center justify-between gap-2 border-b border-[var(--border)] px-3 py-2">
                      <p className="font-mono text-xs text-[var(--text-primary)]">{diff.path}</p>
                      <p className="text-xs text-[var(--text-secondary)]">
                        {diff.diffKind}
                        {diff.diffTruncated ? ' (truncated)' : ''}
                      </p>
                    </div>
                    <pre className="max-h-56 overflow-auto p-3 text-xs text-[var(--text-secondary)]">
                      {diff.diff ?? 'No diff body provided.'}
                    </pre>
                  </article>
                ))}
              </div>
            )}
          </section>

          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
            <h2 className="text-lg font-semibold text-[var(--text-primary)]">Risk Summary</h2>
            {plan.risks.length === 0 ? (
              <p className="mt-3 text-sm text-[var(--text-secondary)]">No risk entries returned by planner.</p>
            ) : (
              <ul className="mt-3 space-y-2">
                {plan.risks.map((risk, index) => (
                  <li className="flex items-start gap-3 rounded-xl border border-[var(--border)] bg-[var(--bg-card)] p-3" key={`${risk.level}:${index.toString()}`}>
                    <RiskPill level={risk.level} />
                    <p className="text-sm text-[var(--text-secondary)]">{risk.message}</p>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
            <h2 className="text-lg font-semibold text-[var(--text-primary)]">Execution</h2>
            <p className="mt-2 text-sm text-[var(--text-secondary)]">
              Choose mode and run. Apply mode requires confirmation. Dry run previews output without writing.
            </p>

            <div className="mt-4 inline-flex rounded-lg border border-[var(--border)] bg-[var(--bg-card)] p-1">
              <button
                className={`rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
                  mode === 'apply-now'
                    ? 'bg-[var(--accent)] text-white'
                    : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
                }`}
                onClick={() => setMode('apply-now')}
                type="button"
              >
                Apply Now
              </button>
              <button
                className={`rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
                  mode === 'dry-run'
                    ? 'bg-[var(--accent)] text-white'
                    : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
                }`}
                onClick={() => setMode('dry-run')}
                type="button"
              >
                Dry Run
              </button>
            </div>

            <div className="mt-4 flex flex-wrap items-center gap-3">
              <Button
                disabled={applying || loadingPlan}
                onClick={() => {
                  if (mode === 'apply-now') {
                    setConfirmOpen(true);
                    return;
                  }
                  handleRunDry();
                }}
                type="button"
              >
                {applying ? 'Running...' : mode === 'apply-now' ? 'Apply Changes' : 'Run Dry Run'}
              </Button>
              <Button
                disabled={loadingPlan || applying}
                onClick={() => {
                  void loadFreshPlan();
                }}
                type="button"
              >
                Refresh Plan
              </Button>
            </div>
          </section>
        </>
      )}

      {applyError && (
        <ErrorBanner
          title="Apply failed"
          message={applyError}
          onRetry={() => {
            if (mode === 'dry-run') {
              handleRunDry();
              return;
            }
            setConfirmOpen(true);
          }}
        />
      )}

      {applyResult && (
        <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <h2 className="text-lg font-semibold text-[var(--text-primary)]">Results</h2>
            <span className={`rounded-md px-2.5 py-1 text-xs font-semibold uppercase tracking-wide ${applyResult.dryRun ? 'bg-sky-500/15 text-sky-300' : 'bg-emerald-500/15 text-emerald-300'}`}>
              {applyResult.dryRun ? 'Dry Run' : 'Applied'}
            </span>
          </div>

          <div className="mt-3 grid gap-3 sm:grid-cols-3">
            <SummaryCard label="Actions Processed" value={applyResult.appliedActions} />
            <SummaryCard label="Changed Files" value={applyResult.changedFiles} />
            <SummaryCard label="Files Written" value={filesWritten} />
          </div>

          <div className="mt-4 space-y-2 rounded-xl border border-[var(--border)] bg-[var(--bg-card)] p-3">
            <p className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)]">Backup locations</p>
            {backupPaths.length === 0 ? (
              <p className="text-sm text-[var(--text-secondary)]">No backup paths reported.</p>
            ) : (
              backupPaths.map((path) => (
                <p className="break-all font-mono text-xs text-[var(--text-primary)]" key={path}>
                  {path}
                </p>
              ))
            )}
          </div>

          {applyResult.warnings.length > 0 && (
            <div className="mt-4 rounded-xl border border-amber-500/30 bg-amber-500/10 p-3">
              <p className="text-xs font-semibold uppercase tracking-wide text-amber-300">Warnings</p>
              <ul className="mt-2 space-y-1 text-sm text-amber-200">
                {applyResult.warnings.map((warning, index) => (
                  <li key={`${warning}:${index.toString()}`}>{warning}</li>
                ))}
              </ul>
            </div>
          )}

          <div className="mt-4 overflow-x-auto rounded-xl border border-[var(--border)]">
            <table className="w-full min-w-[680px] text-left text-sm">
              <thead className="bg-[var(--bg-card)]">
                <tr>
                  <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Path</th>
                  <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Kind</th>
                  <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Status</th>
                  <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Backup</th>
                  <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Message</th>
                </tr>
              </thead>
              <tbody>
                {applyResult.results.map((result) => (
                  <tr className="border-t border-[var(--border)]" key={`${result.path}:${result.kind}`}> 
                    <td className="px-3 py-2 font-mono text-xs text-[var(--text-primary)]">{result.path}</td>
                    <td className="px-3 py-2 text-[var(--text-secondary)]">{result.kind}</td>
                    <td className="px-3 py-2">
                      <span className={`rounded-md px-2 py-1 text-xs font-semibold ${result.success ? 'bg-emerald-500/15 text-emerald-300' : 'bg-rose-500/15 text-rose-300'}`}>
                        {result.success ? 'Success' : 'Failed'}
                      </span>
                    </td>
                    <td className="px-3 py-2 text-[var(--text-secondary)] font-mono text-xs">{result.backupLocation ?? '-'}</td>
                    <td className="px-3 py-2 text-[var(--text-secondary)]">{result.message ?? '-'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div className="mt-5 flex flex-wrap gap-3">
            <Link to="/ops/backups">
              <Button type="button">View Backup</Button>
            </Link>
            <Link to="/ops/diagnostics">
              <Button type="button">Run Doctor</Button>
            </Link>
            <Link to="/dashboard">
              <Button type="button">Back to Dashboard</Button>
            </Link>
          </div>
        </section>
      )}

      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title="Confirm Apply"
        description={`Apply ${plan?.summary.totalActions ?? 0} planned actions. Backup path: ${plan?.summary.backupPath || 'not provided'}. Continue?`}
        confirmLabel="Confirm Apply"
        intent="caution"
        onConfirm={handleConfirmApply}
      />
    </div>
  );
};

function SummaryCard({ label, value }: { label: string; value: number }) {
  return (
    <article className="rounded-xl border border-[var(--border)] bg-[var(--bg-card)] p-3">
      <p className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)]">{label}</p>
      <p className="mt-1 text-2xl font-semibold text-[var(--text-primary)]">{value}</p>
    </article>
  );
}

export default Apply;
