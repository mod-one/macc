import React, { useEffect, useState, useMemo } from 'react';
import {
  getCatalogSkillsAvailable,
  getCatalogSkillsStatus,
  getCatalogSkillsInstalled,
  postCatalogSkillsVerify,
} from '../../api/client';
import type {
  ApiCatalogSkillEntry,
  ApiCatalogSkillStatus,
  ApiSkillLockEntry,
  ApiVerifyFinding,
} from '../../api/models';
import { LoadingSpinner } from '../../components/LoadingSpinner';
import { ErrorBanner } from '../../components/ErrorBanner';
import { EmptyState } from '../../components/EmptyState';
import { Button } from '../../components/Button';
import { StatusBadge } from '../../components/StatusBadge';
import { Icons } from '../../components/NavIcons';
import { surfaceClassName, cn } from '../../components/styles';

// ── Types ─────────────────────────────────────────────────────────────────────

type View = 'available' | 'installed' | 'updates' | 'conflicts' | 'provenance';

// ── Helpers ───────────────────────────────────────────────────────────────────

function statusTone(kind: string): 'active' | 'failed' | 'blocked' | 'todo' {
  switch (kind) {
    case 'clean': return 'active';
    case 'modified':
    case 'missing-files': return 'failed';
    case 'unpinned':
    case 'cache-missing': return 'blocked';
    default: return 'todo';
  }
}

function riskTone(risk: string | null): string {
  switch (risk) {
    case 'low': return 'text-green-400';
    case 'medium': return 'text-yellow-400';
    case 'high': return 'text-red-400';
    default: return 'text-[var(--text-muted)]';
  }
}

function formatRef(ref: string | null): string {
  if (!ref) return '—';
  return ref.length > 9 ? ref.slice(0, 9) : ref;
}

// ── Sub-components ─────────────────────────────────────────────────────────────

function ViewTab({
  label,
  active,
  count,
  onClick,
}: {
  label: string;
  active: boolean;
  count?: number;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        'px-4 py-2 text-sm font-medium border-b-2 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]',
        active
          ? 'border-[var(--accent)] text-[var(--accent)]'
          : 'border-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:border-[var(--border)]',
      )}
    >
      {label}
      {count !== undefined && count > 0 && (
        <span className="ml-2 rounded-full bg-[var(--accent)]/10 px-1.5 py-0.5 text-xs text-[var(--accent)]">
          {count}
        </span>
      )}
    </button>
  );
}

// ── Available view (spec §15.2) ───────────────────────────────────────────────

function AvailableView({
  entries,
  onProvenance,
}: {
  entries: ApiCatalogSkillEntry[];
  onProvenance?: (id: string) => void;
}) {
  const [search, setSearch] = useState('');
  const [toolFilter, setToolFilter] = useState('');

  const allTools = useMemo(() => {
    const set = new Set<string>();
    entries.forEach((e) => e.tools.forEach((t) => set.add(t)));
    return Array.from(set).sort();
  }, [entries]);

  const filtered = useMemo(() => {
    return entries.filter((e) => {
      const matchSearch =
        !search ||
        e.id.includes(search) ||
        e.name.toLowerCase().includes(search.toLowerCase()) ||
        e.tags.some((t) => t.includes(search));
      const matchTool = !toolFilter || e.tools.includes(toolFilter);
      return matchSearch && matchTool;
    });
  }, [entries, search, toolFilter]);

  if (entries.length === 0) {
    return (
      <EmptyState
        title="No catalog skills found"
        description="Configure a catalog source in macc.yaml to see available skills."
        icon={<Icons.Brain />}
      />
    );
  }

  return (
    <div className="flex flex-col gap-4">
      {/* Filters */}
      <div className="flex flex-wrap gap-3">
        <input
          type="search"
          placeholder="Search skills…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-1.5 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]"
        />
        <select
          value={toolFilter}
          onChange={(e) => setToolFilter(e.target.value)}
          className="rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-1.5 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]"
        >
          <option value="">All tools</option>
          {allTools.map((t) => (
            <option key={t} value={t}>{t}</option>
          ))}
        </select>
      </div>

      {/* Table */}
      <div className={cn(surfaceClassName, 'overflow-x-auto')}>
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-[var(--border)] text-left text-xs uppercase tracking-wider text-[var(--text-muted)]">
              <th className="px-4 py-3">Skill</th>
              <th className="px-4 py-3">Tools</th>
              <th className="px-4 py-3">Risk</th>
              <th className="px-4 py-3">Ref</th>
              <th className="px-4 py-3">Tags</th>
              <th className="px-4 py-3">Category</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-[var(--border)]">
            {filtered.map((e) => (
              <tr key={e.id} className="hover:bg-white/5">
                <td className="px-4 py-3">
                  <button
                    className="text-left focus-visible:outline-none"
                    onClick={() => onProvenance?.(e.id)}
                  >
                    <div className="font-medium text-[var(--text-primary)]">{e.name}</div>
                    <div className="font-mono text-xs text-[var(--text-muted)]">{e.id}</div>
                    {e.description && (
                      <div className="mt-0.5 text-xs text-[var(--text-secondary)] line-clamp-1">
                        {e.description}
                      </div>
                    )}
                  </button>
                </td>
                <td className="px-4 py-3">
                  <div className="flex flex-wrap gap-1">
                    {e.tools.length === 0 ? (
                      <span className="text-[var(--text-muted)]">any</span>
                    ) : (
                      e.tools.map((t) => (
                        <span key={t} className="rounded bg-white/5 px-1.5 py-0.5 text-xs">
                          {t}
                        </span>
                      ))
                    )}
                  </div>
                </td>
                <td className="px-4 py-3">
                  <span className={cn('text-xs font-medium', riskTone(e.risk))}>
                    {e.risk ?? '—'}
                  </span>
                </td>
                <td className="px-4 py-3 font-mono text-xs text-[var(--text-muted)]">
                  {e.recommended_ref ?? '—'}
                </td>
                <td className="px-4 py-3">
                  <div className="flex flex-wrap gap-1">
                    {e.tags.slice(0, 3).map((t) => (
                      <span key={t} className="rounded bg-white/5 px-1.5 py-0.5 text-xs text-[var(--text-muted)]">
                        {t}
                      </span>
                    ))}
                  </div>
                </td>
                <td className="px-4 py-3 text-xs text-[var(--text-muted)]">
                  {e.category ?? '—'}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="text-xs text-[var(--text-muted)]">
        {filtered.length} of {entries.length} skill(s). Install via CLI: <code className="font-mono">macc skills install &lt;id&gt; --tool &lt;tool&gt;</code>
      </p>
    </div>
  );
}

// ── Installed view (spec §15.4) ───────────────────────────────────────────────

function InstalledView({
  statuses,
  warnings,
  onProvenance,
}: {
  statuses: ApiCatalogSkillStatus[];
  warnings: string[];
  onProvenance?: (id: string) => void;
}) {
  if (statuses.length === 0) {
    return (
      <EmptyState
        title="No installed catalog skills"
        description="Install a skill with: macc skills install <id> --tool <tool>"
        icon={<Icons.CheckSquare />}
      />
    );
  }

  return (
    <div className="flex flex-col gap-4">
      {warnings.length > 0 && (
        <div className="rounded-lg border border-yellow-500/20 bg-yellow-500/10 px-4 py-3 text-sm text-yellow-400">
          <strong>Warnings:</strong>
          <ul className="mt-1 space-y-0.5 list-disc list-inside">
            {warnings.map((w, i) => <li key={i}>{w}</li>)}
          </ul>
        </div>
      )}
      <div className={cn(surfaceClassName, 'overflow-x-auto')}>
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-[var(--border)] text-left text-xs uppercase tracking-wider text-[var(--text-muted)]">
              <th className="px-4 py-3">Tool</th>
              <th className="px-4 py-3">Skill</th>
              <th className="px-4 py-3">Ref</th>
              <th className="px-4 py-3">Pin</th>
              <th className="px-4 py-3">Status</th>
              <th className="px-4 py-3">Source</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-[var(--border)]">
            {statuses.map((s) => (
              <tr key={`${s.id}-${s.tool}`} className="hover:bg-white/5">
                <td className="px-4 py-3 font-mono text-xs">{s.tool}</td>
                <td className="px-4 py-3">
                  <button
                    className="text-left focus-visible:outline-none"
                    onClick={() => onProvenance?.(s.id)}
                  >
                    <span className="font-medium text-[var(--text-primary)] hover:text-[var(--accent)]">
                      {s.id}
                    </span>
                  </button>
                </td>
                <td className="px-4 py-3 font-mono text-xs text-[var(--text-muted)]">
                  {s.requested_ref ?? '—'}
                </td>
                <td className="px-4 py-3 font-mono text-xs text-[var(--text-muted)]">
                  {s.pinned ? formatRef(s.resolved_ref) : 'unpinned'}
                </td>
                <td className="px-4 py-3">
                  <StatusBadge status={s.kind} tone={statusTone(s.kind)} className="text-xs px-2 py-0.5" />
                </td>
                <td className="px-4 py-3 max-w-xs truncate text-xs text-[var(--text-muted)]">
                  {s.source_url ?? '—'}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="text-xs text-[var(--text-muted)]">
        {statuses.length} installed skill(s). Click a skill name to see provenance.
      </p>
    </div>
  );
}

// ── Updates view (spec §15.5) ─────────────────────────────────────────────────

function UpdatesView({ statuses }: { statuses: ApiCatalogSkillStatus[] }) {
  const needsAttention = statuses.filter(
    (s) => s.kind !== 'clean' && s.kind !== 'not-installed',
  );

  if (needsAttention.length === 0) {
    return (
      <EmptyState
        title="All skills up to date"
        description="No drift, modifications, or update warnings detected."
        icon={<Icons.CheckSquare />}
      />
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {needsAttention.map((s) => (
        <div key={`${s.id}-${s.tool}`} className={cn(surfaceClassName, 'p-4')}>
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="flex items-center gap-2">
                <span className="font-medium text-[var(--text-primary)]">{s.id}</span>
                <span className="text-xs text-[var(--text-muted)]">({s.tool})</span>
                <StatusBadge status={s.kind} tone={statusTone(s.kind)} className="text-xs px-2 py-0.5" />
              </div>
              {s.warnings.length > 0 && (
                <ul className="mt-1 space-y-0.5">
                  {s.warnings.map((w, i) => (
                    <li key={i} className="text-xs text-yellow-400">⚠ {w}</li>
                  ))}
                </ul>
              )}
            </div>
            <div className="shrink-0 text-right text-xs text-[var(--text-muted)]">
              <div>ref: {s.requested_ref ?? '—'}</div>
              <div>pin: {s.pinned ? formatRef(s.resolved_ref) : 'unpinned'}</div>
            </div>
          </div>
          {s.installed_files.length > 0 && (
            <div className="mt-2 border-t border-[var(--border)] pt-2">
              <div className="text-xs font-medium text-[var(--text-muted)] mb-1">Installed files</div>
              <ul className="space-y-0.5">
                {s.installed_files.map((f) => (
                  <li key={f} className="font-mono text-xs text-[var(--text-secondary)]">{f}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      ))}
      <p className="text-xs text-[var(--text-muted)]">
        Run <code className="font-mono">macc skills verify</code> to check all digest drift,
        or <code className="font-mono">macc skills diff &lt;id&gt;</code> to inspect local edits.
      </p>
    </div>
  );
}

// ── Conflicts view (spec §15.6) ───────────────────────────────────────────────

function ConflictsView({ findings }: { findings: ApiVerifyFinding[] }) {
  const conflicts = findings.filter((f) =>
    ['conflict', 'digest-mismatch', 'missing-installed-file'].includes(f.kind),
  );

  if (conflicts.length === 0) {
    return (
      <EmptyState
        title="No conflicts detected"
        description="Run macc skills verify to re-check, or install a skill to see conflict detection in action."
        icon={<Icons.Shield />}
      />
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {conflicts.map((f, i) => (
        <div key={i} className={cn(surfaceClassName, 'border-l-2 border-red-500/50 p-4')}>
          <div className="flex items-start gap-3">
            <span className="mt-0.5 text-red-400">❌</span>
            <div className="flex-1">
              <div className="font-medium text-[var(--text-primary)]">
                {f.skill_id}
                <span className="ml-2 text-xs text-[var(--text-muted)]">({f.tool})</span>
              </div>
              <div className="mt-0.5 font-mono text-xs text-[var(--text-muted)]">{f.kind}</div>
              <div className="mt-1 text-sm text-[var(--text-secondary)]">{f.message}</div>
            </div>
          </div>
        </div>
      ))}
      <p className="text-xs text-[var(--text-muted)]">
        Resolve with <code className="font-mono">macc skills verify</code> and{' '}
        <code className="font-mono">macc skills diff</code>.
      </p>
    </div>
  );
}

// ── Provenance drawer (spec §15.7) ────────────────────────────────────────────

function ProvenanceDrawer({
  entry,
  status,
  onClose,
}: {
  entry: ApiSkillLockEntry;
  status?: ApiCatalogSkillStatus;
  onClose: () => void;
}) {
  return (
    <div className={cn(surfaceClassName, 'p-5')}>
      <div className="flex items-center justify-between mb-4">
        <h3 className="font-semibold text-[var(--text-primary)]">Provenance — {entry.id}</h3>
        <button
          onClick={onClose}
          className="text-[var(--text-muted)] hover:text-[var(--text-primary)] focus-visible:outline-none"
        >
          ✕
        </button>
      </div>
      <dl className="grid gap-2 text-sm">
        {[
          ['Skill', entry.id],
          ['Tool', entry.tool],
          ['Source', entry.source.url ?? '—'],
          ['Requested ref', entry.source.requested_ref ?? '—'],
          ['Resolved SHA', entry.source.resolved_ref ?? '—'],
          ['Package version', entry.package.version ?? '—'],
          ['Manifest digest', entry.package.manifest_digest ?? '—'],
          ['Cache key', entry.cache.cache_key],
          ['Installed at', entry.installed.at],
          ['Status', status?.kind ?? '—'],
          ['Pinned', entry.source.pinned ? 'yes' : 'no'],
        ].map(([label, value]) => (
          <div key={label} className="grid grid-cols-[10rem_1fr] gap-2">
            <dt className="text-[var(--text-muted)]">{label}</dt>
            <dd className="font-mono text-xs text-[var(--text-primary)] break-all">{value}</dd>
          </div>
        ))}
      </dl>
      {entry.installed.targets.length > 0 && (
        <div className="mt-4 border-t border-[var(--border)] pt-4">
          <div className="mb-2 text-sm font-medium text-[var(--text-muted)]">Installed paths</div>
          <ul className="space-y-1">
            {entry.installed.targets.map((t) => (
              <li key={t.dest} className="font-mono text-xs text-[var(--text-secondary)]">
                {t.dest}
                {t.digest && (
                  <span className="ml-2 text-[var(--text-muted)]">({t.digest.slice(0, 16)}…)</span>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

// ── Main page ─────────────────────────────────────────────────────────────────

const SkillsCatalog: React.FC = () => {
  const [view, setView] = useState<View>('available');
  const [available, setAvailable] = useState<ApiCatalogSkillEntry[]>([]);
  const [statuses, setStatuses] = useState<ApiCatalogSkillStatus[]>([]);
  const [statusWarnings, setStatusWarnings] = useState<string[]>([]);
  const [installed, setInstalled] = useState<ApiSkillLockEntry[]>([]);
  const [findings, setFindings] = useState<ApiVerifyFinding[]>([]);
  const [loading, setLoading] = useState(true);
  const [verifying, setVerifying] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const [provenanceId, setProvenanceId] = useState<string | null>(null);

  const fetchAll = async () => {
    setLoading(true);
    setError(null);
    try {
      const [avail, stat, inst] = await Promise.all([
        getCatalogSkillsAvailable(),
        getCatalogSkillsStatus(),
        getCatalogSkillsInstalled(),
      ]);
      setAvailable(avail.skills);
      setStatuses(stat.skills);
      setStatusWarnings(stat.warnings);
      setInstalled(inst.skills);
    } catch (err) {
      setError(err instanceof Error ? err : new Error('Failed to load skills catalog'));
    } finally {
      setLoading(false);
    }
  };

  const runVerify = async () => {
    setVerifying(true);
    try {
      const result = await postCatalogSkillsVerify();
      setFindings(result.findings);
      setView('conflicts');
    } catch (err) {
      setError(err instanceof Error ? err : new Error('Verification failed'));
    } finally {
      setVerifying(false);
    }
  };

  useEffect(() => {
    void fetchAll();
  }, []);

  const provenanceEntry = provenanceId
    ? installed.find((e) => e.id === provenanceId) ?? null
    : null;
  const provenanceStatus = provenanceId
    ? statuses.find((s) => s.id === provenanceId)
    : undefined;

  const needsAttentionCount = statuses.filter(
    (s) => s.kind !== 'clean' && s.kind !== 'not-installed',
  ).length;
  const conflictsCount = findings.filter((f) =>
    ['conflict', 'digest-mismatch', 'missing-installed-file'].includes(f.kind),
  ).length;

  if (loading) {
    return (
      <div className="flex h-64 items-center justify-center">
        <LoadingSpinner />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      {/* Header */}
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <h1 className="text-3xl font-semibold tracking-tight text-[var(--text-primary)]">
            Skills Catalog
          </h1>
          <p className="mt-1 text-sm text-[var(--text-secondary)]">
            Manage catalog skills: available, installed, updates, and conflicts.
          </p>
        </div>
        <div className="flex gap-2">
          <Button onClick={fetchAll} disabled={loading}>
            <Icons.Activity />
            <span className="ml-2">Refresh</span>
          </Button>
          <Button
            className="bg-[var(--accent)] text-white hover:opacity-90"
            onClick={runVerify}
            disabled={verifying || installed.length === 0}
          >
            <Icons.Shield />
            <span className="ml-2">{verifying ? 'Verifying…' : 'Verify'}</span>
          </Button>
        </div>
      </header>

      {error && <ErrorBanner title="Skills Catalog Error" message={error.message} />}

      {/* Tab strip */}
      <div className="flex border-b border-[var(--border)] overflow-x-auto">
        <ViewTab label="Available" active={view === 'available'} count={available.length} onClick={() => setView('available')} />
        <ViewTab label="Installed" active={view === 'installed'} count={installed.length} onClick={() => setView('installed')} />
        <ViewTab label="Updates" active={view === 'updates'} count={needsAttentionCount} onClick={() => setView('updates')} />
        <ViewTab label="Conflicts" active={view === 'conflicts'} count={conflictsCount} onClick={() => setView('conflicts')} />
        {provenanceEntry && (
          <ViewTab label="Provenance" active={view === 'provenance'} onClick={() => setView('provenance')} />
        )}
      </div>

      {/* Content */}
      <div className="flex flex-col gap-4">
        {view === 'available' && (
          <AvailableView
            entries={available}
            onProvenance={(id) => {
              setProvenanceId(id);
              setView('provenance');
            }}
          />
        )}
        {view === 'installed' && (
          <InstalledView
            statuses={statuses}
            warnings={statusWarnings}
            onProvenance={(id) => {
              setProvenanceId(id);
              setView('provenance');
            }}
          />
        )}
        {view === 'updates' && <UpdatesView statuses={statuses} />}
        {view === 'conflicts' && <ConflictsView findings={findings} />}
        {view === 'provenance' && provenanceEntry && (
          <ProvenanceDrawer
            entry={provenanceEntry}
            status={provenanceStatus}
            onClose={() => {
              setView('installed');
              setProvenanceId(null);
            }}
          />
        )}
        {view === 'provenance' && !provenanceEntry && (
          <EmptyState
            title="No skill selected"
            description="Click a skill name in the Installed or Available views to see its provenance."
            icon={<Icons.Search />}
          />
        )}
      </div>

      {/* CLI hint */}
      <div className={cn(surfaceClassName, 'p-4 text-xs text-[var(--text-muted)]')}>
        <strong className="text-[var(--text-secondary)]">CLI commands:</strong>{' '}
        <code className="font-mono">macc skills available</code> ·{' '}
        <code className="font-mono">macc skills status</code> ·{' '}
        <code className="font-mono">macc skills install &lt;id&gt; --tool &lt;tool&gt;</code> ·{' '}
        <code className="font-mono">macc skills verify</code> ·{' '}
        <code className="font-mono">macc skills diff</code> ·{' '}
        <code className="font-mono">macc skills prune</code>
      </div>
    </div>
  );
};

export default SkillsCatalog;
