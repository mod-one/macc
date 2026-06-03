import React, { useEffect, useState, useMemo } from 'react';
import { getDoctorReport, runDoctorFix } from '../../api/client';
import type { ApiDoctorReport, ApiDiagnosticFinding, ApiDoctorIssue } from '../../api/models';
import { ConfirmDialog } from '../../components/ConfirmDialog';
import { IssueCard } from '../../components/IssueCard';
import { LoadingSpinner } from '../../components/LoadingSpinner';
import { ErrorBanner } from '../../components/ErrorBanner';
import { Icons } from '../../components/NavIcons';
import { Button } from '../../components/Button';
import { EmptyState } from '../../components/EmptyState';
import { KpiCard } from '../../components/KpiCard';

// ── Normalisation helpers ─────────────────────────────────────────────────────

function ensureDoctorIssues(value: unknown): ApiDoctorIssue[] {
  return Array.isArray(value)
    ? value.filter(
        (issue): issue is ApiDoctorIssue => Boolean(issue) && typeof issue === 'object',
      )
    : [];
}

function ensureDiagnosticFindings(value: unknown): ApiDiagnosticFinding[] {
  return Array.isArray(value)
    ? value.filter(
        (f): f is ApiDiagnosticFinding => Boolean(f) && typeof f === 'object' && 'id' in f,
      )
    : [];
}

function ensureSeverityMap(value: unknown): Record<string, number> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
  return Object.fromEntries(
    Object.entries(value).filter((e): e is [string, number] => typeof e[1] === 'number'),
  );
}

function normalizeDoctorReport(report: ApiDoctorReport): ApiDoctorReport {
  return {
    ...report,
    healthScore: typeof report.healthScore === 'number' ? report.healthScore : 0,
    issuesBySeverity: ensureSeverityMap(report.issuesBySeverity),
    issues: ensureDoctorIssues(report.issues),
    findings: ensureDiagnosticFindings(report.findings),
  };
}

// ── Severity helpers ──────────────────────────────────────────────────────────

function findingTone(severity: string) {
  switch (severity) {
    case 'error': return 'failed' as const;
    case 'warning': return 'blocked' as const;
    case 'ok': return 'active' as const;
    default: return 'todo' as const;
  }
}

function legacyTone(severity: string) {
  switch (severity) {
    case 'error': return 'failed' as const;
    case 'warning': return 'blocked' as const;
    default: return 'todo' as const;
  }
}

// ── Category labels ───────────────────────────────────────────────────────────

const CATEGORY_LABELS: Record<string, string> = {
  git: 'Git',
  coordinator: 'Coordinator',
  tools: 'Tools',
  tasks: 'Tasks',
  worktrees: 'Worktrees',
  project: 'Project',
};

// ── Component ─────────────────────────────────────────────────────────────────

const Diagnostics: React.FC = () => {
  const [report, setReport] = useState<ApiDoctorReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const [selectedCategory, setSelectedCategory] = useState<string>('All');

  const [fixConfirmOpen, setFixConfirmOpen] = useState(false);
  const [fixing, setFixing] = useState(false);
  const [fixResult, setFixResult] = useState<{ status: string; message: string } | null>(null);

  const fetchReport = async () => {
    setLoading(true);
    setError(null);
    setFixResult(null);
    try {
      const data = normalizeDoctorReport(await getDoctorReport());
      setReport(data);
    } catch (err) {
      setError(err instanceof Error ? err : new Error('Failed to fetch doctor report'));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchReport();
  }, []);

  const handleFixAll = async () => {
    setFixing(true);
    try {
      const res = await runDoctorFix();
      setFixResult({ status: res.status || 'success', message: res.message || 'Fix complete' });
      await fetchReport();
    } catch (err) {
      setError(err instanceof Error ? err : new Error('Failed to run fixes'));
    } finally {
      setFixing(false);
      setFixConfirmOpen(false);
    }
  };

  // Prefer the new `findings` array; fall back to legacy `issues`.
  const useFindings = Boolean(report?.findings && report.findings.length > 0);

  // Categories from findings (stable category field) or legacy toolId.
  const categories = useMemo(() => {
    if (!report) return [];
    const cats = new Set<string>(['All']);
    if (useFindings) {
      report.findings?.forEach((f) => cats.add(f.category));
    } else {
      report.issues.forEach((i) => cats.add(i.toolId ?? 'System'));
    }
    return Array.from(cats).sort((a, b) =>
      a === 'All' ? -1 : b === 'All' ? 1 : a.localeCompare(b),
    );
  }, [report, useFindings]);

  const filteredFindings = useMemo(() => {
    if (!report?.findings) return [];
    if (selectedCategory === 'All') return report.findings;
    return report.findings.filter((f) => f.category === selectedCategory);
  }, [report, selectedCategory]);

  const filteredLegacyIssues = useMemo(() => {
    if (!report) return [];
    if (selectedCategory === 'All') return report.issues;
    return report.issues.filter((i) => (i.toolId ?? 'System') === selectedCategory);
  }, [report, selectedCategory]);

  const criticalCount = report?.issuesBySeverity['error'] ?? 0;
  const warningCount = report?.issuesBySeverity['warning'] ?? 0;
  const fixableCount = report?.findings?.filter((f) => f.fixAvailable).length ?? 0;

  if (loading && !report) {
    return (
      <div className="flex h-64 items-center justify-center">
        <LoadingSpinner />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      {/* Header */}
      <header className="flex flex-wrap items-end justify-between gap-4 rounded-[2rem] border border-slate-200 bg-white p-6 shadow-sm dark:border-white/10 dark:bg-black/20">
        <div>
          <h1 className="text-5xl font-semibold tracking-tight text-slate-950 dark:text-white">
            Diagnostics
          </h1>
          <p className="mt-3 text-base text-slate-600 dark:text-white/60">
            System health, issue detection, and remediation.
          </p>
        </div>
        <div className="flex gap-3">
          <Button onClick={fetchReport} disabled={loading || fixing}>
            <Icons.Activity />
            <span className="ml-2">Refresh</span>
          </Button>
          <Button
            className="bg-[var(--accent)] text-white hover:opacity-90"
            onClick={() => setFixConfirmOpen(true)}
            disabled={fixableCount === 0 || loading || fixing}
          >
            <Icons.Wrench />
            <span className="ml-2">Fix All Safe</span>
          </Button>
        </div>
      </header>

      {error && <ErrorBanner title="Diagnostics Error" message={error.message} />}

      {fixResult && (
        <div
          className={`rounded-lg border p-4 ${
            fixResult.status === 'ok'
              ? 'border-green-500/20 bg-green-500/10 text-green-700 dark:text-green-400'
              : 'border-blue-500/20 bg-blue-500/10 text-blue-700 dark:text-blue-400'
          }`}
        >
          <h3 className="font-semibold capitalize">{fixResult.status}</h3>
          <p className="mt-1 text-sm">{fixResult.message}</p>
        </div>
      )}

      {report != null && (
        <>
          {/* KPI strip */}
          <section className="grid gap-4 sm:grid-cols-3">
            <KpiCard
              title="Health Score"
              value={`${report.healthScore}%`}
              delta={report.healthScore >= 90 ? 1 : -1}
              deltaLabel={report.healthScore >= 90 ? 'Healthy' : 'Needs attention'}
            />
            <KpiCard
              title="Critical Issues"
              value={criticalCount.toString()}
              deltaLabel="error severity"
            />
            <KpiCard
              title="Warnings"
              value={warningCount.toString()}
              deltaLabel="warning severity"
            />
          </section>

          {/* Readiness banner (only when findings available) */}
          {useFindings && report.ready === false && (
            <div className="rounded-lg border border-red-500/20 bg-red-500/10 px-4 py-3 text-sm text-red-700 dark:text-red-400">
              <span className="font-semibold">Not ready to dispatch a task.</span>{' '}
              Resolve the blocking issues below, then refresh.
            </div>
          )}
          {useFindings && report.ready === true && (
            <div className="rounded-lg border border-green-500/20 bg-green-500/10 px-4 py-3 text-sm text-green-700 dark:text-green-400">
              <span className="font-semibold">✅ Ready to dispatch a task.</span>
            </div>
          )}

          <div className="flex flex-col gap-6 md:flex-row">
            {/* Category sidebar */}
            <aside className="w-full shrink-0 space-y-2 md:w-56">
              <h3 className="mb-4 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
                Categories
              </h3>
              <nav className="flex flex-col gap-1">
                {categories.map((cat) => {
                  const label = cat === 'All' ? 'All' : (CATEGORY_LABELS[cat] ?? cat);
                  const count =
                    cat === 'All'
                      ? (useFindings ? report.findings?.length : report.issues.length) ?? 0
                      : useFindings
                        ? (report.findings?.filter((f) => f.category === cat).length ?? 0)
                        : report.issues.filter((i) => (i.toolId ?? 'System') === cat).length;

                  return (
                    <button
                      key={cat}
                      onClick={() => setSelectedCategory(cat)}
                      className={`flex items-center justify-between rounded-md px-3 py-2 text-sm transition-colors ${
                        selectedCategory === cat
                          ? 'bg-[var(--accent)]/10 text-[var(--accent)]'
                          : 'text-[var(--text-secondary)] hover:bg-white/5'
                      }`}
                    >
                      <span className="font-medium">{label}</span>
                      <span className="rounded-full bg-black/20 px-2 py-0.5 text-xs text-[var(--text-muted)]">
                        {count}
                      </span>
                    </button>
                  );
                })}
              </nav>
            </aside>

            {/* Issue list */}
            <main className="flex-1 space-y-4">
              {useFindings ? (
                filteredFindings.length === 0 ? (
                  <EmptyState
                    title="No Issues in This Category"
                    description="Select a different category or refresh."
                    icon={<Icons.CheckSquare />}
                  />
                ) : (
                  filteredFindings.map((f) => (
                    <IssueCard
                      key={f.id}
                      title={f.title}
                      code={f.id}
                      severity={f.severity}
                      severityTone={findingTone(f.severity)}
                      whyItMatters={f.message || undefined}
                      fix={f.recommendedAction}
                      actions={
                        f.fixAvailable
                          ? [
                              {
                                label: 'Apply Safe Fix',
                                onClick: () => setFixConfirmOpen(true),
                              },
                            ]
                          : []
                      }
                    />
                  ))
                )
              ) : filteredLegacyIssues.length === 0 ? (
                <EmptyState
                  title="No Issues Found"
                  description="Your system is healthy and fully configured."
                  icon={<Icons.CheckSquare />}
                />
              ) : (
                filteredLegacyIssues.map((issue, idx) => (
                  <IssueCard
                    key={`${issue.name}-${idx}`}
                    title={issue.name}
                    code={issue.target}
                    severity={String(issue.severity)}
                    severityTone={legacyTone(String(issue.severity))}
                    currentState={issue.status}
                    expectedState={issue.message ?? 'Expected to be healthy'}
                    actions={[]}
                  />
                ))
              )}
            </main>
          </div>
        </>
      )}

      <ConfirmDialog
        open={fixConfirmOpen}
        onOpenChange={setFixConfirmOpen}
        title="Apply Safe Fixes"
        description={`Run automated safe fixes for up to ${fixableCount} issue(s). This may modify configuration files or install missing dependencies.`}
        confirmLabel={fixing ? 'Applying…' : 'Confirm and Fix'}
        onConfirm={handleFixAll}
        intent="caution"
      />
    </div>
  );
};

export default Diagnostics;
