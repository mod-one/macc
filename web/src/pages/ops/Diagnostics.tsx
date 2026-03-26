import React, { useEffect, useState, useMemo } from 'react';
import { getDoctorReport, runDoctorFix } from '../../api/client';
import type { ApiDoctorReport } from '../../api/models';
import { ConfirmDialog } from '../../components/ConfirmDialog';
import { IssueCard } from '../../components/IssueCard';
import { LoadingSpinner } from '../../components/LoadingSpinner';
import { ErrorBanner } from '../../components/ErrorBanner';
import { Icons } from '../../components/NavIcons';
import { Button } from '../../components/Button';
import { EmptyState } from '../../components/EmptyState';
import { KpiCard } from '../../components/KpiCard';

function ensureDoctorIssues(value: unknown): ApiDoctorReport['issues'] {
  return Array.isArray(value) ? value.filter((issue): issue is ApiDoctorReport['issues'][number] => Boolean(issue) && typeof issue === 'object') : [];
}

function ensureSeverityMap(value: unknown): Record<string, number> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return {};
  }

  return Object.fromEntries(
    Object.entries(value).filter((entry): entry is [string, number] => typeof entry[1] === 'number'),
  );
}

function normalizeDoctorReport(report: ApiDoctorReport): ApiDoctorReport {
  return {
    ...report,
    healthScore: typeof report.healthScore === 'number' ? report.healthScore : 0,
    issuesBySeverity: ensureSeverityMap(report.issuesBySeverity),
    issues: ensureDoctorIssues(report.issues),
  };
}

const Diagnostics: React.FC = () => {
  const [report, setReport] = useState<ApiDoctorReport | null>(null);
  const [loading, setLoading] = useState<boolean>(true);
  const [error, setError] = useState<Error | null>(null);
  const [selectedCategory, setSelectedCategory] = useState<string>('All');
  
  // Bulk Fix state
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
      setFixResult({
        status: res.status || 'success',
        message: res.message || 'Fix operation completed',
      });
      // Refresh report after fixing
      await fetchReport();
    } catch (err) {
      setError(err instanceof Error ? err : new Error('Failed to run fixes'));
    } finally {
      setFixing(false);
      setFixConfirmOpen(false);
    }
  };

  const categories = useMemo(() => {
    if (!report) return [];
    const cats = new Set<string>();
    cats.add('All');
    report.issues.forEach(issue => {
      cats.add(issue.toolId || 'System');
    });
    return Array.from(cats).sort();
  }, [report]);

  const filteredIssues = useMemo(() => {
    if (!report) return [];
    if (selectedCategory === 'All') return report.issues;
    return report.issues.filter(i => (i.toolId || 'System') === selectedCategory);
  }, [report, selectedCategory]);

  const getSeverityTone = (severity: string) => {
    switch (severity) {
      case 'error': return 'failed';
      case 'warning': return 'blocked';
      default: return 'todo';
    }
  };

  const criticalCount = report?.issuesBySeverity['error'] || 0;
  const warningCount = report?.issuesBySeverity['warning'] || 0;
  const fixableIssuesCount = report?.issues.filter(i => i.severity === 'error' || i.severity === 'warning').length || 0;

  if (loading && !report) {
    return (
      <div className="flex h-64 items-center justify-center">
        <LoadingSpinner />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-wrap items-end justify-between gap-4 rounded-[2rem] border border-slate-200 bg-white p-6 shadow-sm dark:border-white/10 dark:bg-black/20">
        <div>
          <h1 className="text-5xl font-semibold tracking-tight text-slate-950 dark:text-white">Diagnostics</h1>
          <p className="mt-3 text-base text-slate-600 dark:text-white/60">
            System health overview, issue detection, and automated remediation.
          </p>
        </div>
        <div className="flex gap-3">
          <Button className="border-white/20 hover:bg-white/10" onClick={fetchReport} disabled={loading || fixing}>
            <Icons.Activity />
            <span className="ml-2">Refresh</span>
          </Button>
          <Button 
            className="bg-[var(--accent)] text-white hover:opacity-90"
            onClick={() => setFixConfirmOpen(true)}
            disabled={fixableIssuesCount === 0 || loading || fixing}
          >
            <Icons.Wrench />
            <span className="ml-2">Fix All Safe</span>
          </Button>
        </div>
      </header>

      {error && <ErrorBanner title="Diagnostics Error" message={error.message} />}

      {fixResult && (
        <div className={`rounded-lg border p-4 ${fixResult.status === 'success' ? 'border-green-500/20 bg-green-500/10 text-green-700 dark:text-green-400' : 'border-blue-500/20 bg-blue-500/10 text-blue-700 dark:text-blue-400'}`}>
          <h3 className="font-semibold capitalize">{fixResult.status}</h3>
          <p className="text-sm mt-1">{fixResult.message}</p>
        </div>
      )}

      {report && (
        <>
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

          <div className="flex flex-col gap-6 md:flex-row">
            <aside className="w-full shrink-0 space-y-2 md:w-64">
              <h3 className="mb-4 text-sm font-semibold uppercase tracking-wider text-slate-500 dark:text-white/50">
                Categories
              </h3>
              <nav className="flex flex-col gap-1">
                {categories.map(cat => {
                  const count = cat === 'All' 
                    ? report.issues.length 
                    : report.issues.filter(i => (i.toolId || 'System') === cat).length;
                  
                  return (
                    <button
                      key={cat}
                      onClick={() => setSelectedCategory(cat)}
                      className={`flex items-center justify-between rounded-md px-3 py-2 text-sm transition-colors ${
                        selectedCategory === cat 
                          ? 'bg-blue-50 text-blue-700 dark:bg-white/10 dark:text-white' 
                          : 'text-slate-600 hover:bg-slate-50 dark:text-white/60 dark:hover:bg-white/5'
                      }`}
                    >
                      <span className="font-medium">{cat}</span>
                      <span className="rounded-full bg-slate-100 px-2 py-0.5 text-xs text-slate-500 dark:bg-black/30 dark:text-white/50">
                        {count}
                      </span>
                    </button>
                  );
                })}
              </nav>
            </aside>

            <main className="flex-1 space-y-4">
              {filteredIssues.length === 0 ? (
                <EmptyState
                  title="No Issues Found"
                  description="Your system is healthy and fully configured."
                  icon={<Icons.CheckSquare />}
                />
              ) : (
                filteredIssues.map((issue, idx) => (
                  <IssueCard
                    key={`${issue.name}-${idx}`}
                    title={issue.name}
                    code={issue.target}
                    severity={issue.severity}
                    severityTone={getSeverityTone(issue.severity)}
                    currentState={issue.status}
                    expectedState={issue.message || 'Expected to be healthy'}
                    actions={[
                      {
                        label: 'Safe Fix',
                        onClick: () => console.log('Safe fix issue', issue.name),
                        disabled: issue.severity !== 'error' && issue.severity !== 'warning'
                      },
                      {
                        label: 'Manual Fix',
                        onClick: () => console.log('Manual fix issue', issue.name),
                      },
                      {
                        label: 'Ignore',
                        onClick: () => console.log('Ignore issue', issue.name),
                      }
                    ]}
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
        description={`You are about to run automated safe fixes for up to ${fixableIssuesCount} issues. This may modify configuration files or install missing dependencies.`}
        confirmLabel={fixing ? 'Applying...' : 'Confirm and Fix'}
        onConfirm={handleFixAll}
        intent="caution"
      />
    </div>
  );
};

export default Diagnostics;
