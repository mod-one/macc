import React from 'react';
import { useNavigate } from 'react-router-dom';
import { ApiClientError, getConfig, getWorktrees, runPlan } from '../api/client';
import type { ApiPlanResponse, ApiRiskLevel, ApiScope, ApiWorktree } from '../api/models';
import { Button, ErrorBanner, LoadingSpinner, StatusBadge } from '../components';
import type { StatusTone } from '../components/StatusBadge';

function formatError(error: unknown): string {
  if (error instanceof ApiClientError) {
    return `${error.envelope.error.message} (${error.envelope.error.code})`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return 'Unexpected error.';
}

const RISK_TONE: Record<ApiRiskLevel, StatusTone> = {
  safe: 'merged',
  caution: 'blocked',
  dangerous: 'failed',
};

function computePlanHash(plan: ApiPlanResponse): string {
  const payload = JSON.stringify({
    summary: plan.summary,
    files: plan.files.map((f) => `${f.path}:${f.kind}:${f.riskLevel}`),
  });
  let hash = 0;
  for (let i = 0; i < payload.length; i++) {
    hash = ((hash << 5) - hash + payload.charCodeAt(i)) | 0;
  }
  return (hash >>> 0).toString(16).padStart(8, '0');
}

const Plan: React.FC = () => {
  const navigate = useNavigate();

  const [enabledTools, setEnabledTools] = React.useState<string[]>([]);
  const [selectedTools, setSelectedTools] = React.useState<string[]>([]);
  const [scope, setScope] = React.useState<ApiScope>('project');
  const [offline, setOffline] = React.useState(false);

  const [worktrees, setWorktrees] = React.useState<ApiWorktree[]>([]);
  const [selectedWorktrees, setSelectedWorktrees] = React.useState<string[]>([]);

  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [plan, setPlan] = React.useState<ApiPlanResponse | null>(null);
  const [expandedDiffs, setExpandedDiffs] = React.useState<Set<string>>(new Set());

  React.useEffect(() => {
    let cancelled = false;
    getConfig()
      .then((config) => {
        if (!cancelled) {
          setEnabledTools(config.enabledTools);
        }
      })
      .catch(() => {
        /* config fetch is best-effort */
      });
    getWorktrees()
      .then((wts) => {
        if (!cancelled) {
          setWorktrees(wts);
        }
      })
      .catch(() => {
        /* worktree fetch is best-effort */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleRunPlan = React.useCallback(async () => {
    setLoading(true);
    setError(null);
    setPlan(null);
    setExpandedDiffs(new Set());

    try {
      const response = await runPlan({
        scope,
        tools: selectedTools.length > 0 ? selectedTools : undefined,
        worktrees: selectedWorktrees.length > 0 ? selectedWorktrees : undefined,
        offline: offline || undefined,
        includeDiff: true,
        explain: true,
      });
      setPlan(response);
    } catch (err) {
      setError(formatError(err));
    } finally {
      setLoading(false);
    }
  }, [scope, selectedTools, selectedWorktrees, offline]);

  const toggleTool = React.useCallback((toolId: string) => {
    setSelectedTools((prev) =>
      prev.includes(toolId) ? prev.filter((t) => t !== toolId) : [...prev, toolId],
    );
  }, []);

  const toggleWorktree = React.useCallback((id: string) => {
    setSelectedWorktrees((prev) =>
      prev.includes(id) ? prev.filter((w) => w !== id) : [...prev, id],
    );
  }, []);

  const toggleDiff = React.useCallback((path: string) => {
    setExpandedDiffs((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, []);

  const diffsByPath = React.useMemo(() => {
    if (!plan) return new Map<string, string | null>();
    const map = new Map<string, string | null>();
    for (const diff of plan.diffs) {
      map.set(diff.path, diff.diff);
    }
    return map;
  }, [plan]);

  const riskCounts = React.useMemo(() => {
    if (!plan) return { safe: 0, caution: 0, dangerous: 0 };
    const counts = { safe: 0, caution: 0, dangerous: 0 };
    for (const file of plan.files) {
      if (file.riskLevel in counts) {
        counts[file.riskLevel as ApiRiskLevel]++;
      }
    }
    return counts;
  }, [plan]);

  const planHash = React.useMemo(() => {
    if (!plan) return null;
    return computePlanHash(plan);
  }, [plan]);

  const handleApplyThisPlan = React.useCallback(() => {
    if (!plan) return;
    void navigate('/apply', { state: { plan } });
  }, [navigate, plan]);

  return (
    <div className="flex flex-col gap-6 pb-8">
      <header className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6 shadow-sm">
        <h1 className="text-3xl font-semibold tracking-tight text-[var(--text-primary)]">Plan</h1>
        <p className="mt-2 max-w-3xl text-sm text-[var(--text-secondary)]">
          Configure plan options and preview the ActionPlan with diffs and risk levels. Only preview
          — no files are written.
        </p>
      </header>

      {/* Configuration form */}
      <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
        <h2 className="text-lg font-semibold text-[var(--text-primary)]">Configuration</h2>

        {/* Tool selector */}
        {enabledTools.length > 0 && (
          <div className="mt-4">
            <p className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)]">
              Tools (leave unchecked for all)
            </p>
            <div className="mt-2 flex flex-wrap gap-3">
              {enabledTools.map((toolId) => (
                <label
                  className="flex items-center gap-2 rounded-lg border border-[var(--border)] bg-[var(--bg-card)] px-3 py-2 text-sm text-[var(--text-primary)] cursor-pointer hover:bg-[var(--bg-primary)] transition-colors"
                  key={toolId}
                >
                  <input
                    checked={selectedTools.includes(toolId)}
                    className="accent-[var(--accent)]"
                    onChange={() => toggleTool(toolId)}
                    type="checkbox"
                  />
                  {toolId}
                </label>
              ))}
            </div>
          </div>
        )}

        {/* Scope */}
        <div className="mt-4">
          <p className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)]">
            Scope
          </p>
          <div className="mt-2 inline-flex rounded-lg border border-[var(--border)] bg-[var(--bg-card)] p-1">
            <button
              className={`rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
                scope === 'project'
                  ? 'bg-[var(--accent)] text-white'
                  : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
              }`}
              onClick={() => setScope('project')}
              type="button"
            >
              Project
            </button>
            <button
              className={`rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
                scope === 'user'
                  ? 'bg-[var(--accent)] text-white'
                  : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
              }`}
              onClick={() => setScope('user')}
              type="button"
            >
              User
            </button>
          </div>
        </div>

        {/* Worktree selector */}
        {worktrees.length > 0 && scope === 'project' && (
          <div className="mt-4">
            <p className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)]">
              Worktrees (leave unchecked for all)
            </p>
            <div className="mt-2 flex flex-wrap gap-3">
              {worktrees.map((wt) => (
                <label
                  className="flex items-center gap-2 rounded-lg border border-[var(--border)] bg-[var(--bg-card)] px-3 py-2 text-sm text-[var(--text-primary)] cursor-pointer hover:bg-[var(--bg-primary)] transition-colors"
                  key={wt.id}
                >
                  <input
                    checked={selectedWorktrees.includes(wt.id)}
                    className="accent-[var(--accent)]"
                    onChange={() => toggleWorktree(wt.id)}
                    type="checkbox"
                  />
                  {wt.slug ?? wt.id}
                </label>
              ))}
            </div>
          </div>
        )}

        {/* Toggles */}
        <div className="mt-4 flex flex-wrap gap-4">
          <label className="flex items-center gap-2 text-sm text-[var(--text-primary)] cursor-pointer">
            <input
              checked={offline}
              className="accent-[var(--accent)]"
              onChange={(e) => setOffline(e.target.checked)}
              type="checkbox"
            />
            Offline mode
          </label>
        </div>

        {/* Run button */}
        <div className="mt-5">
          <Button disabled={loading} onClick={() => void handleRunPlan()} type="button">
            {loading ? 'Running Plan...' : 'Run Plan'}
          </Button>
        </div>
      </section>

      {/* Loading state */}
      {loading && (
        <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
          <LoadingSpinner label="Running plan" />
          <p className="mt-2 text-sm text-[var(--text-secondary)]">
            Generating plan preview with diffs...
          </p>
        </section>
      )}

      {/* Error state */}
      {error && (
        <ErrorBanner
          title="Plan failed"
          message={error}
          onRetry={() => void handleRunPlan()}
        />
      )}

      {/* Plan results */}
      {plan && (
        <>
          {/* Summary + hash */}
          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
            <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
              <h2 className="text-lg font-semibold text-[var(--text-primary)]">Plan Summary</h2>
              {planHash && (
                <span className="rounded-md border border-[var(--border)] bg-[var(--bg-card)] px-2.5 py-1 font-mono text-xs text-[var(--text-secondary)]">
                  Hash: {planHash}
                </span>
              )}
            </div>

            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              <SummaryCard label="Total Actions" value={plan.summary.totalActions} />
              <SummaryCard label="Files Write" value={plan.summary.filesWrite} />
              <SummaryCard label="Files Merge" value={plan.summary.filesMerge} />
              <SummaryCard label="Consent Required" value={plan.summary.consentRequired} />
              <SummaryCard label="Backup Required" value={plan.summary.backupRequired} />
              <SummaryCard label="Plan Files" value={plan.files.length} />
            </div>

            {/* Risk breakdown */}
            <div className="mt-4 flex flex-wrap gap-3">
              <StatusBadge status={`Safe ${riskCounts.safe}`} tone="merged" />
              <StatusBadge status={`Caution ${riskCounts.caution}`} tone="blocked" />
              <StatusBadge status={`Dangerous ${riskCounts.dangerous}`} tone="failed" />
            </div>
          </section>

          {/* File list table with expandable diffs */}
          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
            <h2 className="text-lg font-semibold text-[var(--text-primary)]">Planned Files</h2>
            {plan.files.length === 0 ? (
              <p className="mt-3 text-sm text-[var(--text-secondary)]">
                No file actions in the current plan.
              </p>
            ) : (
              <div className="mt-3 overflow-x-auto rounded-xl border border-[var(--border)]">
                <table className="w-full min-w-[720px] text-left text-sm">
                  <thead className="bg-[var(--bg-card)]">
                    <tr>
                      <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Path</th>
                      <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Action</th>
                      <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Risk</th>
                      <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Consent</th>
                      <th className="px-3 py-2 font-semibold text-[var(--text-secondary)]">Diff</th>
                    </tr>
                  </thead>
                  <tbody>
                    {plan.files.map((file) => {
                      const hasDiff = diffsByPath.has(file.path);
                      const isExpanded = expandedDiffs.has(file.path);
                      return (
                        <React.Fragment key={`${file.path}:${file.kind}`}>
                          <tr className="border-t border-[var(--border)]">
                            <td className="px-3 py-2 font-mono text-xs text-[var(--text-primary)]">
                              {file.path}
                            </td>
                            <td className="px-3 py-2 text-[var(--text-secondary)]">{file.kind}</td>
                            <td className="px-3 py-2">
                              <StatusBadge status={file.riskLevel} tone={RISK_TONE[file.riskLevel]} />
                            </td>
                            <td className="px-3 py-2 text-[var(--text-secondary)]">
                              {file.consentRequired ? 'Required' : 'No'}
                            </td>
                            <td className="px-3 py-2">
                              {hasDiff && (
                                <button
                                  className="text-xs font-medium text-[var(--accent)] hover:underline"
                                  onClick={() => toggleDiff(file.path)}
                                  type="button"
                                >
                                  {isExpanded ? 'Hide' : 'Show'}
                                </button>
                              )}
                            </td>
                          </tr>
                          {isExpanded && (
                            <tr className="border-t border-[var(--border)] bg-[var(--bg-card)]">
                              <td className="p-0" colSpan={5}>
                                <pre className="max-h-72 overflow-auto p-3 text-xs text-[var(--text-secondary)]">
                                  {diffsByPath.get(file.path) ?? 'No diff body provided.'}
                                </pre>
                              </td>
                            </tr>
                          )}
                        </React.Fragment>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
          </section>

          {/* Risk summary */}
          {plan.risks.length > 0 && (
            <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
              <h2 className="text-lg font-semibold text-[var(--text-primary)]">Risk Summary</h2>
              <ul className="mt-3 space-y-2">
                {plan.risks.map((risk, index) => (
                  <li
                    className="flex items-start gap-3 rounded-xl border border-[var(--border)] bg-[var(--bg-card)] p-3"
                    key={`${risk.level}:${index.toString()}`}
                  >
                    <StatusBadge status={risk.level} tone={RISK_TONE[risk.level]} />
                    <p className="text-sm text-[var(--text-secondary)]">{risk.message}</p>
                  </li>
                ))}
              </ul>
            </section>
          )}

          {/* Consent requirements */}
          {plan.consents.length > 0 && (
            <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
              <h2 className="text-lg font-semibold text-[var(--text-primary)]">
                Consent Requirements
              </h2>
              <div className="mt-3 space-y-3">
                {plan.consents.map((consent) => (
                  <article
                    className="rounded-xl border border-[var(--border)] bg-[var(--bg-card)] p-3"
                    key={consent.id}
                  >
                    <div className="flex flex-wrap items-center gap-2">
                      <StatusBadge status={consent.classification} tone={RISK_TONE[consent.classification]} />
                      <span className="text-xs uppercase text-[var(--text-muted)]">
                        {consent.scope}
                      </span>
                    </div>
                    <p className="mt-2 text-sm text-[var(--text-secondary)]">{consent.message}</p>
                    {consent.paths.length > 0 && (
                      <ul className="mt-2 space-y-0.5">
                        {consent.paths.map((p) => (
                          <li className="font-mono text-xs text-[var(--text-secondary)]" key={p}>
                            {p}
                          </li>
                        ))}
                      </ul>
                    )}
                  </article>
                ))}
              </div>
            </section>
          )}

          {/* Actions */}
          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5">
            <div className="flex flex-wrap items-center gap-3">
              <Button onClick={handleApplyThisPlan} type="button">
                Apply This Plan
              </Button>
              <Button disabled={loading} onClick={() => void handleRunPlan()} type="button">
                Re-run Plan
              </Button>
            </div>
            {planHash && (
              <p className="mt-3 font-mono text-xs text-[var(--text-muted)]">
                Config: scope={scope}, tools=
                {selectedTools.length > 0 ? selectedTools.join(',') : 'all'}, offline=
                {String(offline)} | Plan hash: {planHash}
              </p>
            )}
          </section>
        </>
      )}
    </div>
  );
};

function SummaryCard({ label, value }: { label: string; value: number }) {
  return (
    <article className="rounded-xl border border-[var(--border)] bg-[var(--bg-card)] p-3">
      <p className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)]">
        {label}
      </p>
      <p className="mt-1 text-2xl font-semibold text-[var(--text-primary)]">{value}</p>
    </article>
  );
}

export default Plan;
