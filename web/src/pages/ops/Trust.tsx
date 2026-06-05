import React, { useState, useEffect } from 'react';
import { getTrust } from '../../api/client';
import type { ApiTrustSummary } from '../../api/models';
import { Button, LoadingSpinner, ErrorBanner } from '../../components';
import { RefreshIcon, CheckCircleIcon, AlertTriangleIcon } from '../../components/icons';

function errorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}

const Trust: React.FC = () => {
  const [trust, setTrust] = useState<ApiTrustSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [copySuccess, setCopySuccess] = useState(false);

  const fetchTrust = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await getTrust();
      setTrust(data);
    } catch (err) {
      setError(errorMessage(err, 'Failed to load trust summary'));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchTrust();
  }, []);

  const handleCopyPath = async (path: string) => {
    try {
      await navigator.clipboard.writeText(path);
      setCopySuccess(true);
      setTimeout(() => setCopySuccess(false), 2000);
    } catch {
      // ignore
    }
  };

  if (loading && !trust) {
    return (
      <div className="flex flex-1 items-center justify-center py-20">
        <LoadingSpinner size="lg" />
      </div>
    );
  }

  const state = trust?.state || 'caution';
  
  // Custom headers/cards gradients based on state
  const stateThemes = {
    trusted: {
      bg: 'bg-emerald-50 border-emerald-200',
      text: 'text-emerald-700',
      heading: 'text-emerald-950',
      badge: 'bg-emerald-500 text-white',
      accent: 'emerald',
      gradient: 'bg-[radial-gradient(circle_at_top_left,_rgba(16,185,129,0.16),_transparent_38%),linear-gradient(135deg,#ffffff,#f0fdf4)]',
      desc: 'Everything is local, pinned, backed up, and scanned. Project is in a secure, verifiable state.',
    },
    caution: {
      bg: 'bg-amber-50 border-amber-200',
      text: 'text-amber-700',
      heading: 'text-amber-950',
      badge: 'bg-amber-500 text-white',
      accent: 'amber',
      gradient: 'bg-[radial-gradient(circle_at_top_left,_rgba(245,158,11,0.16),_transparent_38%),linear-gradient(135deg,#ffffff,#fffbeb)]',
      desc: 'Some configurations are unpinned, or terminal capability / user-scope writes are active.',
    },
    risky: {
      bg: 'bg-rose-50 border-rose-200',
      text: 'text-rose-700',
      heading: 'text-rose-950',
      badge: 'bg-rose-500 text-white',
      accent: 'rose',
      gradient: 'bg-[radial-gradient(circle_at_top_left,_rgba(244,63,94,0.16),_transparent_38%),linear-gradient(135deg,#ffffff,#fff1f2)]',
      desc: 'Potential issues found. Host is exposed, backups are missing/incomplete, or secrets were detected.',
    },
    blocked: {
      bg: 'bg-red-50 border-red-200',
      text: 'text-red-700',
      heading: 'text-red-950',
      badge: 'bg-red-700 text-white',
      accent: 'red',
      gradient: 'bg-[radial-gradient(circle_at_top_left,_rgba(220,38,38,0.16),_transparent_38%),linear-gradient(135deg,#ffffff,#fef2f2)]',
      desc: 'Execution blocked due to safety policy violation or unsafe write path detection.',
    },
  }[state];

  return (
    <section className="mx-auto flex w-full max-w-6xl flex-col gap-6 text-slate-700">
      <header className={`rounded-[2rem] border p-6 shadow-sm transition-all ${stateThemes.bg} ${stateThemes.gradient}`}>
        <div className="flex flex-col gap-5 lg:flex-row lg:items-center lg:justify-between">
          <div className="max-w-2xl">
            <div className="flex items-center gap-3">
              <p className={`text-sm font-semibold uppercase tracking-[0.2em] ${stateThemes.text}`}>
                Security Center
              </p>
              <span className={`rounded-full px-3 py-1 text-xs font-bold uppercase tracking-wider ${stateThemes.badge}`}>
                {state}
              </span>
            </div>
            <h1 className="mb-3 mt-2 text-5xl font-semibold tracking-tight text-slate-950">Trust & Safety</h1>
            <p className="max-w-xl text-base leading-7 text-slate-600">
              {stateThemes.desc}
            </p>
          </div>
          
          <Button
            onClick={fetchTrust}
            className="flex items-center gap-2 self-start lg:self-center"
          >
            <RefreshIcon className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
            Refresh Status
          </Button>
        </div>
      </header>

      {error && <ErrorBanner message={error} onRetry={fetchTrust} />}

      {trust && (
        <div className="grid gap-6 md:grid-cols-2 lg:grid-cols-3">
          {/* Server Exposure */}
          <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-sm flex flex-col justify-between">
            <div>
              <div className="flex items-center justify-between mb-4">
                <h3 className="font-bold text-slate-900">Server Exposure</h3>
                {trust.local_only ? (
                  <span className="rounded-full bg-emerald-50 px-2 py-0.5 text-xs font-semibold text-emerald-700 border border-emerald-100 flex items-center gap-1">
                    <CheckCircleIcon className="h-3.5 w-3.5" /> Local Only
                  </span>
                ) : (
                  <span className="rounded-full bg-amber-50 px-2 py-0.5 text-xs font-semibold text-amber-700 border border-amber-100 flex items-center gap-1">
                    <AlertTriangleIcon className="h-3.5 w-3.5" /> Exposed
                  </span>
                )}
              </div>
              <p className="text-sm text-slate-500 mb-4">
                MACC is bound to <code className="bg-slate-100 px-1 py-0.5 rounded text-slate-800">{trust.server_exposure}</code>. 
                {trust.local_only 
                  ? " It will refuse any connections originating from outside localhost."
                  : " Caution: the service is configured to accept remote traffic."}
              </p>
            </div>
            <div className="pt-4 border-t border-slate-100 flex justify-between text-xs font-semibold text-slate-500">
              <span>Binding</span>
              <span>{trust.server_exposure}</span>
            </div>
          </div>

          {/* Terminal PTY Access */}
          <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-sm flex flex-col justify-between">
            <div>
              <div className="flex items-center justify-between mb-4">
                <h3 className="font-bold text-slate-900">Terminal Access</h3>
                {trust.terminal_enabled ? (
                  <span className="rounded-full bg-amber-50 px-2 py-0.5 text-xs font-semibold text-amber-700 border border-amber-100 flex items-center gap-1">
                    <AlertTriangleIcon className="h-3.5 w-3.5" /> Enabled
                  </span>
                ) : (
                  <span className="rounded-full bg-emerald-50 px-2 py-0.5 text-xs font-semibold text-emerald-700 border border-emerald-100 flex items-center gap-1">
                    <CheckCircleIcon className="h-3.5 w-3.5" /> Disabled
                  </span>
                )}
              </div>
              <p className="text-sm text-slate-500 mb-2">
                Controls whether subagents can spawn PTY bash shell sessions.
              </p>
              <div className="mb-2">
                <span className="text-xs font-semibold text-slate-400 block mb-1">Allowed Roots:</span>
                <div className="max-h-20 overflow-y-auto flex flex-col gap-1">
                  {trust.allowed_roots.map((root, i) => (
                    <code key={i} className="text-[11px] bg-slate-50 p-1 rounded block truncate text-slate-600" title={root}>
                      {root}
                    </code>
                  ))}
                </div>
              </div>
            </div>
            <div className="pt-4 border-t border-slate-100 flex justify-between text-xs font-semibold text-slate-500">
              <span>PTY Capability</span>
              <span>{trust.terminal_enabled ? 'Active' : 'Inactive'}</span>
            </div>
          </div>

          {/* User-Level Writes */}
          <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-sm flex flex-col justify-between">
            <div>
              <div className="flex items-center justify-between mb-4">
                <h3 className="font-bold text-slate-900">User Scope Writes</h3>
                {trust.user_level_writes === 0 ? (
                  <span className="rounded-full bg-emerald-50 px-2 py-0.5 text-xs font-semibold text-emerald-700 border border-emerald-100 flex items-center gap-1">
                    <CheckCircleIcon className="h-3.5 w-3.5" /> None
                  </span>
                ) : (
                  <span className="rounded-full bg-amber-50 px-2 py-0.5 text-xs font-semibold text-amber-700 border border-amber-100 flex items-center gap-1">
                    <AlertTriangleIcon className="h-3.5 w-3.5" /> Active
                  </span>
                )}
              </div>
              <p className="text-sm text-slate-500 mb-4">
                Shows the number of planned edits targeting directories outside the current project root, such as global config files.
              </p>
            </div>
            <div className="pt-4 border-t border-slate-100 flex justify-between text-xs font-semibold text-slate-500">
              <span>Planned External Writes</span>
              <span className="font-bold text-slate-700">{trust.user_level_writes} files</span>
            </div>
          </div>

          {/* Backup Strategy */}
          <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-sm flex flex-col justify-between">
            <div>
              <div className="flex items-center justify-between mb-4">
                <h3 className="font-bold text-slate-900">Backups</h3>
                {trust.backups_ready ? (
                  <span className="rounded-full bg-emerald-50 px-2 py-0.5 text-xs font-semibold text-emerald-700 border border-emerald-100 flex items-center gap-1">
                    <CheckCircleIcon className="h-3.5 w-3.5" /> Ready
                  </span>
                ) : (
                  <span className="rounded-full bg-rose-50 px-2 py-0.5 text-xs font-semibold text-rose-700 border border-rose-100 flex items-center gap-1">
                    <AlertTriangleIcon className="h-3.5 w-3.5" /> Missing
                  </span>
                )}
              </div>
              <p className="text-sm text-slate-500 mb-4">
                Verify backup status. MACC captures an automatic rollback point in <code className="bg-slate-100 px-1 py-0.5 rounded text-slate-800">.macc/backups/</code> before mutating files.
              </p>
            </div>
            <div className="pt-4 border-t border-slate-100 flex justify-between text-xs font-semibold text-slate-500">
              <span>Backup Directory</span>
              <span className="text-sky-600 hover:underline cursor-pointer" onClick={() => window.location.hash = '#/ops/backups'}>
                View History
              </span>
            </div>
          </div>

          {/* Catalog Pinned */}
          <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-sm flex flex-col justify-between">
            <div>
              <div className="flex items-center justify-between mb-4">
                <h3 className="font-bold text-slate-900">Catalog Integrity</h3>
                {trust.catalog_pinned ? (
                  <span className="rounded-full bg-emerald-50 px-2 py-0.5 text-xs font-semibold text-emerald-700 border border-emerald-100 flex items-center gap-1">
                    <CheckCircleIcon className="h-3.5 w-3.5" /> Pinned
                  </span>
                ) : (
                  <span className="rounded-full bg-amber-50 px-2 py-0.5 text-xs font-semibold text-amber-700 border border-amber-100 flex items-center gap-1">
                    <AlertTriangleIcon className="h-3.5 w-3.5" /> Unpinned
                  </span>
                )}
              </div>
              <p className="text-sm text-slate-500 mb-4">
                Ensure all remote skill or MCP package catalogs reference strict commit checksums/SHAs to guard against upstream tampering.
              </p>
            </div>
            <div className="pt-4 border-t border-slate-100 flex justify-between text-xs font-semibold text-slate-500">
              <span>Verification</span>
              <span>{trust.catalog_pinned ? 'Checksums Validated' : 'Verify Config'}</span>
            </div>
          </div>

          {/* Secrets Redacted */}
          <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-sm flex flex-col justify-between">
            <div>
              <div className="flex items-center justify-between mb-4">
                <h3 className="font-bold text-slate-900">Secrets Scanning</h3>
                {trust.secrets_redacted ? (
                  <span className="rounded-full bg-emerald-50 px-2 py-0.5 text-xs font-semibold text-emerald-700 border border-emerald-100 flex items-center gap-1">
                    <CheckCircleIcon className="h-3.5 w-3.5" /> Redacted
                  </span>
                ) : (
                  <span className="rounded-full bg-rose-50 px-2 py-0.5 text-xs font-semibold text-rose-700 border border-rose-100 flex items-center gap-1">
                    <AlertTriangleIcon className="h-3.5 w-3.5" /> Leak Risk
                  </span>
                )}
              </div>
              <p className="text-sm text-slate-500 mb-4">
                Checks if credentials, private keys, or API tokens have been accidentally exposed in settings files.
              </p>
            </div>
            <div className="pt-4 border-t border-slate-100 flex justify-between text-xs font-semibold text-slate-500">
              <span>Risk Warning</span>
              <span>{trust.secrets_redacted ? 'No Leak Detected' : 'Sensitive Info Stored!'}</span>
            </div>
          </div>
        </div>
      )}

      {/* Audit Logs */}
      {trust && (
        <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-sm">
          <div className="flex items-center justify-between mb-4">
            <h3 className="font-bold text-slate-900">Audit Logs & History</h3>
            <span className="rounded-full bg-slate-100 px-2.5 py-0.5 text-xs font-semibold text-slate-600 border border-slate-200">
              Enabled
            </span>
          </div>
          <p className="text-sm text-slate-500 mb-4">
            All mutating workspace activities and coordination steps are logged securely for compliance and reproducibility.
          </p>
          <div className="flex flex-col sm:flex-row items-stretch sm:items-center gap-3 bg-slate-50 p-4 rounded-xl border border-slate-200">
            <code className="text-xs text-slate-600 font-mono flex-1 break-all truncate select-all">{trust.audit_log}</code>
            <div className="flex gap-2 shrink-0">
              <Button 
                onClick={() => handleCopyPath(trust.audit_log)}
                className="text-xs px-2.5 py-1 bg-white/5 border border-white/10 hover:bg-white/10"
              >
                {copySuccess ? 'Copied!' : 'Copy Path'}
              </Button>
              <Button 
                onClick={() => window.location.hash = '#/ops/logs'}
                className="text-xs px-2.5 py-1 bg-[var(--accent)] text-white hover:opacity-90 border-none"
              >
                View Logs
              </Button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
};

export default Trust;
