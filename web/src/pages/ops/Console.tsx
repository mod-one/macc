/**
 * Console — merged coordinator control + live worker monitor
 * Replaces the old Console (control panel) and Live (stream wall) pages.
 */
import React, {
  useCallback, useEffect, useMemo, useRef, useState,
} from 'react';
import { useNavigate } from 'react-router-dom';
import { useCoordinatorStore } from '../../store';
import {
  getRegistryTasks, getWorktrees,
  ApiClientError,
  buildUrl,
} from '../../api/client';
import { resolveApiBaseUrl } from '../../api/config';
import type {
  ApiCoordinatorAction,
  ApiRegistryTask,
  ApiWorktree,
} from '../../api/models';
import { StatusBadge, type StatusTone } from '../../components/StatusBadge';
import { ConfirmDialog } from '../../components/ConfirmDialog';
import { ToolCooldownPanel } from '../../components/ToolCooldownPanel';
import { TakeoverNotificationToast } from '../../components/TakeoverNotificationToast';
import { useIsOwner } from '../../stores/ownershipStore';
import {
  PauseIcon, PlayIcon, CopyIcon, DownloadIcon,
  RefreshIcon,
} from '../../components/icons';
import { cn } from '../../components/styles';

/* ── Constants ───────────────────────────────────────────────── */
const POLL_MS      = 5_000;
const MAX_LOG_LINES = 400;

/* ── Helpers ─────────────────────────────────────────────────── */
function statusTone(s: string | null | undefined): StatusTone {
  if (!s) return 'todo';
  const l = s.toLowerCase();
  if (l === 'in_progress' || l === 'running' || l === 'active') return 'active';
  if (l === 'merged' || l === 'success' || l === 'complete') return 'merged';
  if (l === 'failed' || l === 'error') return 'failed';
  if (l === 'blocked') return 'blocked';
  if (l === 'paused') return 'paused';
  return 'todo';
}

function isActive(s: string | null | undefined): boolean {
  if (!s) return false;
  const l = s.toLowerCase();
  return l === 'running' || l === 'active' || l === 'in_progress';
}

function isError(s: string | null | undefined): boolean {
  if (!s) return false;
  const l = s.toLowerCase();
  return l === 'failed' || l === 'error';
}

function fmtTime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  return `${m}:${String(s).padStart(2, '0')}`;
}

/* ── Per-worktree live log stream hook ───────────────────────── */
function useWorktreeStream(worktreeId: string, paused: boolean) {
  const [logs, setLogs] = useState<string[]>([]);
  const [conn, setConn] = useState<'connecting' | 'open' | 'error'>('connecting');
  const pausedRef = useRef(paused);
  useEffect(() => { pausedRef.current = paused; }, [paused]);

  useEffect(() => {
    let active = true;
    let src: EventSource | null = null;

    const connect = () => {
      if (!active) return;
      const base = resolveApiBaseUrl(undefined);
      const url = new URL(
        buildUrl(`/worktrees/${encodeURIComponent(worktreeId)}/logs`, base),
        base ?? window.location.origin,
      );
      src = new EventSource(url.toString());
      if (active) setConn('connecting');

      src.addEventListener('open', () => { if (active) setConn('open'); });
      src.addEventListener('error', () => { if (active) setConn('error'); });
      src.addEventListener('log_line', (e: MessageEvent<string>) => {
        if (!active || pausedRef.current) return;
        try {
          const p = JSON.parse(e.data);
          const msg = typeof p.message === 'string' ? p.message : JSON.stringify(p);
          setLogs((prev) => {
            const next = [...prev, msg];
            return next.length > MAX_LOG_LINES ? next.slice(next.length - MAX_LOG_LINES) : next;
          });
        } catch { /* ignore */ }
      });
    };

    connect();
    return () => { active = false; src?.close(); };
  }, [worktreeId]);

  return { logs, conn };
}

/* ── Worker tile ─────────────────────────────────────────────── */
const WorkerTile: React.FC<{
  worktree: ApiWorktree;
  task?: ApiRegistryTask;
  index: number;
}> = ({ worktree, task, index }) => {
  const navigate = useNavigate();
  const [paused, setPaused] = useState(false);
  const { logs, conn } = useWorktreeStream(worktree.id, paused);
  const logRef = useRef<HTMLDivElement>(null);
  const userScrolled = useRef(false);

  const handleCopy = useCallback(() => {
    void navigator.clipboard.writeText(logs.join('\n'));
  }, [logs]);

  const handleDownload = useCallback(() => {
    const blob = new Blob([logs.join('\n')], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${worktree.id}-logs.txt`;
    a.click();
    URL.revokeObjectURL(url);
  }, [logs, worktree.id]);

  // Auto-scroll unless user has scrolled up
  useEffect(() => {
    if (!paused && !userScrolled.current && logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [logs, paused]);

  const active = isActive(worktree.status);
  const errored = isError(worktree.status);

  const connLabel =
    worktree.status === 'failed' ? 'Failed' :
    conn === 'open' ? 'Live' :
    conn === 'connecting' ? 'Connecting' : 'Reconnecting';

  const connTone: StatusTone =
    errored ? 'failed' :
    conn === 'open' ? 'active' : 'todo';

  return (
    <article
      className="worker-tile"
      style={{
        display: 'flex',
        flexDirection: 'column',
        borderRadius: 'var(--radius-lg)',
        border: `1px solid ${active ? 'oklch(0.60 0.15 255 / 0.3)' : errored ? 'oklch(0.62 0.22 25 / 0.3)' : 'var(--border)'}`,
        background: 'var(--bg-card)',
        overflow: 'hidden',
        cursor: 'pointer',
        animationDelay: `${index * 50}ms`,
        animation: 'tile-enter 200ms cubic-bezier(0.16, 1, 0.3, 1) both',
      }}
      onClick={() => navigate(`/ops/worker/${encodeURIComponent(worktree.id)}`)}
    >
      {/* Tile header */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '10px 12px 8px',
          gap: 8,
          borderBottom: '1px solid var(--border-subtle)',
          background: active ? 'oklch(0.17 0.04 255)' : errored ? 'oklch(0.16 0.05 25)' : 'var(--bg-elevated)',
        }}
      >
        <div style={{ minWidth: 0, flex: 1 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            {active && (
              <span className="dot-pulse" style={{ width: 6, height: 6, borderRadius: '50%', background: 'var(--success)', flexShrink: 0 }} />
            )}
            <span
              style={{
                fontFamily: 'var(--font-mono)',
                fontSize: '11px',
                fontWeight: 500,
                color: 'var(--text-primary)',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              {worktree.id}
            </span>
          </div>
          {task && (
            <p
              style={{
                fontSize: '11px',
                color: 'var(--text-muted)',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
                marginTop: 2,
              }}
            >
              {task.title ?? task.id}
            </p>
          )}
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexShrink: 0 }}>
          <span
            style={{
              fontSize: '10px',
              fontFamily: 'var(--font-mono)',
              color: 'var(--text-muted)',
              background: 'var(--bg-secondary)',
              padding: '2px 6px',
              borderRadius: 4,
              border: '1px solid var(--border)',
            }}
          >
            {worktree.tool ?? 'unknown'}
          </span>
          <StatusBadge status={connLabel} tone={connTone} className="text-[10px] px-1.5 py-0.5" />
        </div>
      </div>

      {/* Log pane */}
      <div
        ref={logRef}
        role="log"
        aria-label={`${worktree.id} log stream`}
        onScroll={(e) => {
          const el = e.currentTarget;
          userScrolled.current = el.scrollTop + el.clientHeight < el.scrollHeight - 24;
        }}
        onClick={(e) => e.stopPropagation()}
        style={{
          flex: 1,
          minHeight: 120,
          maxHeight: 160,
          overflowY: 'auto',
          padding: '8px 10px',
          fontFamily: 'var(--font-mono)',
          fontSize: '10px',
          lineHeight: 1.6,
          color: 'var(--text-secondary)',
          background: 'oklch(0.07 0 0)',
        }}
      >
        {logs.length === 0 ? (
          <span style={{ color: 'var(--text-muted)' }}>Waiting for output…</span>
        ) : (
          logs.slice(-60).map((line, i) => (
            <div key={i} className="log-line" style={{ wordBreak: 'break-all' }}>
              {line}
            </div>
          ))
        )}
      </div>

      {/* Tile footer */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'flex-end',
          gap: 4,
          padding: '4px 8px',
          borderTop: '1px solid var(--border-subtle)',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <TileIconBtn aria-label={paused ? 'Resume' : 'Pause'} onClick={() => setPaused((v) => !v)}>
          {paused ? <PlayIcon className="h-3 w-3" /> : <PauseIcon className="h-3 w-3" />}
        </TileIconBtn>
        <TileIconBtn aria-label="Copy logs" onClick={handleCopy}>
          <CopyIcon className="h-3 w-3" />
        </TileIconBtn>
        <TileIconBtn aria-label="Download logs" onClick={handleDownload}>
          <DownloadIcon className="h-3 w-3" />
        </TileIconBtn>
      </div>
    </article>
  );
};

const TileIconBtn: React.FC<{
  children: React.ReactNode;
  onClick: () => void;
  'aria-label': string;
}> = ({ children, onClick, 'aria-label': label }) => (
  <button
    type="button"
    aria-label={label}
    onClick={onClick}
    style={{
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      width: 24, height: 24,
      background: 'none',
      border: 'none',
      borderRadius: 'var(--radius-sm)',
      color: 'var(--text-muted)',
      cursor: 'pointer',
      transition: 'color 100ms, background 100ms',
    }}
    onMouseEnter={(e) => { const el = e.currentTarget; el.style.color = 'var(--text-primary)'; el.style.background = 'var(--bg-elevated)'; }}
    onMouseLeave={(e) => { const el = e.currentTarget; el.style.color = 'var(--text-muted)'; el.style.background = 'none'; }}
  >
    {children}
  </button>
);

/* ── Task row ────────────────────────────────────────────────── */
const TaskRow: React.FC<{ task: ApiRegistryTask; isFeatured?: boolean }> = ({ task, isFeatured }) => {
  const navigate = useNavigate();
  const tone = statusTone(task.state);
  const phase = task.currentPhase;
  const err = task.lastErrorCode ?? task.lastError;

  return (
    <button
      type="button"
      onClick={() => navigate(`/ops/registry/tasks/${encodeURIComponent(task.id)}`)}
      className="task-row"
      style={{
        display: 'grid',
        gridTemplateColumns: '1fr auto',
        alignItems: 'center',
        gap: 8,
        width: '100%',
        padding: isFeatured ? '9px 12px' : '7px 12px',
        background: isFeatured ? 'var(--accent-bg)' : 'transparent',
        border: isFeatured ? '1px solid oklch(0.60 0.15 255 / 0.25)' : '1px solid transparent',
        borderRadius: 'var(--radius-md)',
        cursor: 'pointer',
        textAlign: 'left',
        transition: 'background 100ms, border-color 100ms',
        marginBottom: isFeatured ? 2 : 1,
      }}
      onMouseEnter={(e) => {
        if (!isFeatured) (e.currentTarget as HTMLElement).style.background = 'var(--bg-elevated)';
      }}
      onMouseLeave={(e) => {
        if (!isFeatured) (e.currentTarget as HTMLElement).style.background = 'transparent';
      }}
    >
      <div style={{ minWidth: 0 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 1 }}>
          {isFeatured && (
            <span className="dot-pulse" style={{ width: 5, height: 5, borderRadius: '50%', background: 'var(--accent)', flexShrink: 0 }} />
          )}
          <span
            style={{
              fontFamily: 'var(--font-mono)',
              fontSize: '10px',
              color: isFeatured ? 'var(--accent)' : 'var(--text-muted)',
              flexShrink: 0,
            }}
          >
            {task.id}
          </span>
          {task.tool && (
            <span style={{ fontSize: '10px', color: 'var(--text-muted)' }}>· {task.tool}</span>
          )}
          {phase && (
            <span style={{ fontSize: '10px', color: 'var(--text-muted)' }}>· {phase}</span>
          )}
        </div>
        <p
          style={{
            fontSize: 'var(--text-sm)',
            color: isFeatured ? 'var(--text-primary)' : 'var(--text-secondary)',
            fontWeight: isFeatured ? 500 : 400,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {task.title ?? task.id}
        </p>
        {err && (
          <p style={{ fontSize: '10px', color: 'var(--error)', marginTop: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {err}
          </p>
        )}
      </div>
      <StatusBadge status={task.state} tone={tone} className="text-[10px] px-1.5 py-0.5 shrink-0" />
    </button>
  );
};

/* ── Action button ───────────────────────────────────────────── */
const ActionBtn: React.FC<{
  label: string;
  onClick: () => void;
  disabled?: boolean;
  variant?: 'primary' | 'default' | 'danger' | 'ghost';
  loading?: boolean;
  title?: string;
}> = ({ label, onClick, disabled, variant = 'default', loading, title }) => {
  const styles: React.CSSProperties = {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 6,
    padding: '7px 14px',
    borderRadius: 'var(--radius-md)',
    fontSize: 'var(--text-sm)',
    fontWeight: 500,
    cursor: disabled ? 'not-allowed' : 'pointer',
    transition: 'background 120ms, border-color 120ms, transform 80ms',
    opacity: disabled ? 0.45 : 1,
    border: '1px solid transparent',
    width: '100%',
  };

  if (variant === 'primary') {
    styles.background = 'var(--accent)';
    styles.color = '#fff';
    styles.borderColor = 'var(--accent)';
  } else if (variant === 'danger') {
    styles.background = 'oklch(0.62 0.22 25 / 0.12)';
    styles.color = 'var(--error)';
    styles.borderColor = 'oklch(0.62 0.22 25 / 0.35)';
  } else if (variant === 'ghost') {
    styles.background = 'none';
    styles.color = 'var(--text-muted)';
    styles.fontSize = '11px';
  } else {
    styles.background = 'var(--bg-elevated)';
    styles.color = 'var(--text-primary)';
    styles.borderColor = 'var(--border)';
  }

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      style={styles}
      onMouseEnter={(e) => {
        if (!disabled) {
          const el = e.currentTarget;
          if (variant === 'primary') el.style.background = 'var(--accent-hover)';
          else if (variant === 'danger') el.style.background = 'oklch(0.62 0.22 25 / 0.2)';
          else if (variant !== 'ghost') el.style.background = 'var(--bg-elevated)';
        }
      }}
      onMouseLeave={(e) => {
        const el = e.currentTarget;
        if (variant === 'primary') el.style.background = 'var(--accent)';
        else if (variant === 'danger') el.style.background = 'oklch(0.62 0.22 25 / 0.12)';
        else if (variant === 'ghost') el.style.background = 'none';
        else el.style.background = 'var(--bg-elevated)';
      }}
    >
      {loading && <RefreshIcon className="h-3.5 w-3.5 animate-spin" />}
      {label}
    </button>
  );
};

/* ── Stat row ────────────────────────────────────────────────── */
const StatRow: React.FC<{ label: string; value: number; tone?: StatusTone }> = ({ label, value, tone }) => (
  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '4px 0' }}>
    <span style={{ fontSize: 'var(--text-sm)', color: 'var(--text-secondary)' }}>{label}</span>
    <span
      style={{
        fontFamily: 'var(--font-mono)',
        fontSize: 'var(--text-sm)',
        fontWeight: 500,
        color:
          tone === 'active' ? 'var(--accent)' :
          tone === 'failed' ? 'var(--error)' :
          tone === 'blocked' ? 'var(--warning)' :
          tone === 'merged' ? 'var(--success)' :
          'var(--text-primary)',
      }}
    >
      {value}
    </span>
  </div>
);

/* ── Section label ───────────────────────────────────────────── */
const SectionLabel: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <p style={{ fontSize: '10px', fontWeight: 500, color: 'var(--text-muted)', letterSpacing: '0.04em', marginBottom: 8, userSelect: 'none' }}>
    {children}
  </p>
);

/* ── Main page ───────────────────────────────────────────────── */
const Console: React.FC = () => {
  const navigate = useNavigate();
  const status = useCoordinatorStore((s) => s.status);
  const loadStatus = useCoordinatorStore((s) => s.loadStatus);
  const runAction = useCoordinatorStore((s) => s.runAction);
  const pendingAction = useCoordinatorStore((s) => s.pendingAction);
  const isOwner = useIsOwner();

  const [tasks, setTasks] = useState<ApiRegistryTask[]>([]);
  const [worktrees, setWorktrees] = useState<ApiWorktree[]>([]);
  const [ownerErr, setOwnerErr] = useState<string | null>(null);
  const [stopConfirm, setStopConfirm] = useState(false);
  const [workerFilter, setWorkerFilter] = useState<'active' | 'errors' | 'all'>('active');
  const [toolFilter, setToolFilter] = useState('all');
  const [taskFilter, setTaskFilter] = useState<'active' | 'queue' | 'all'>('all');
  const [elapsed, setElapsed] = useState(0);
  const startRef = useRef(Date.now());

  // Poll data
  useEffect(() => {
    const fetchAll = async () => {
      await loadStatus().catch(() => null);
      await Promise.all([
        getRegistryTasks().then(setTasks).catch(() => null),
        getWorktrees().then(setWorktrees).catch(() => null),
      ]);
    };
    void fetchAll();
    const id = setInterval(fetchAll, POLL_MS);
    return () => clearInterval(id);
  }, [loadStatus]);

  // Elapsed clock
  useEffect(() => {
    const id = setInterval(() => setElapsed(Math.floor((Date.now() - startRef.current) / 1000)), 1000);
    return () => clearInterval(id);
  }, []);

  const isBusy = pendingAction !== null;
  const viewerHint = !isOwner ? 'Viewer mode — request control to take actions' : undefined;

  const handleAction = async (action: ApiCoordinatorAction) => {
    try {
      await runAction(action);
    } catch (err) {
      if (err instanceof ApiClientError && err.status === 403) {
        setOwnerErr('Viewer mode — request control, then retry.');
        setTimeout(() => setOwnerErr(null), 5000);
      }
    }
  };

  // Derived data
  const tools = useMemo(() => {
    const ts = new Set(worktrees.map((w) => w.tool).filter(Boolean) as string[]);
    return ['all', ...Array.from(ts).sort()];
  }, [worktrees]);

  const filteredWorkers = useMemo(() => {
    return worktrees.filter((w) => {
      if (workerFilter === 'active' && !isActive(w.status)) return false;
      if (workerFilter === 'errors' && !isError(w.status)) return false;
      if (toolFilter !== 'all' && w.tool !== toolFilter) return false;
      return true;
    });
  }, [worktrees, workerFilter, toolFilter]);

  // Map worktree id → task (by worktree path)
  const worktreeTaskMap = useMemo(() => {
    const map = new Map<string, ApiRegistryTask>();
    for (const task of tasks) {
      if (task.worktree?.worktreePath) {
        // worktree.id is the path slug; match by checking if path ends with id
        for (const wt of worktrees) {
          if (task.worktree.worktreePath.includes(wt.id) || wt.id.includes(task.id)) {
            map.set(wt.id, task);
          }
        }
      }
    }
    return map;
  }, [tasks, worktrees]);

  const sortedTasks = useMemo(() => {
    const ordered = [...tasks].sort((a, b) => {
      const rankA = a.state === 'in_progress' ? 0 : a.state === 'todo' ? 1 : 2;
      const rankB = b.state === 'in_progress' ? 0 : b.state === 'todo' ? 1 : 2;
      if (rankA !== rankB) return rankA - rankB;
      return new Date(b.updatedAt ?? 0).getTime() - new Date(a.updatedAt ?? 0).getTime();
    });

    if (taskFilter === 'active') return ordered.filter((t) => t.state === 'in_progress');
    if (taskFilter === 'queue') return ordered.filter((t) => t.state === 'todo' || t.state === 'blocked');
    return ordered.slice(0, 30);
  }, [tasks, taskFilter]);

  const coordRunning = (status?.active ?? 0) > 0 && !status?.paused;
  const coordPaused  = status?.paused;

  const coordLabel =
    coordPaused  ? 'Paused'  :
    coordRunning ? 'Running' : 'Idle';

  return (
    <>
      <style>{`
        @keyframes dot-breathe {
          0%, 100% { opacity: 1; transform: scale(1); }
          50% { opacity: 0.5; transform: scale(0.7); }
        }
        @keyframes tile-enter {
          from { opacity: 0; transform: translateY(6px); }
          to   { opacity: 1; transform: translateY(0); }
        }
        @keyframes log-appear {
          from { opacity: 0; transform: translateY(3px); }
          to   { opacity: 1; transform: translateY(0); }
        }
        .dot-pulse {
          animation: dot-breathe 2s ease-in-out infinite;
        }
        .log-line {
          animation: log-appear 150ms ease-out both;
        }
        @media (prefers-reduced-motion: reduce) {
          .dot-pulse, .log-line, .worker-tile { animation: none !important; }
        }
        .filter-chip {
          padding: 3px 10px;
          border-radius: 999px;
          border: 1px solid var(--border);
          font-size: 11px;
          font-weight: 500;
          cursor: pointer;
          transition: background 100ms, color 100ms, border-color 100ms;
          background: none;
          color: var(--text-muted);
        }
        .filter-chip.active {
          background: var(--accent-bg);
          border-color: oklch(0.60 0.15 255 / 0.4);
          color: var(--accent);
        }
        .filter-chip:hover:not(.active) {
          background: var(--bg-elevated);
          color: var(--text-primary);
        }
        .divider {
          height: 1px;
          background: var(--border-subtle);
          margin: 14px 0;
        }
      `}</style>

      <TakeoverNotificationToast />

      {/* ── Header ─────────────────────────────────────────── */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 16,
          marginBottom: 20,
          flexWrap: 'wrap',
        }}
      >
        {/* Status indicator */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          {coordRunning && (
            <span className="dot-pulse" style={{ width: 8, height: 8, borderRadius: '50%', background: 'var(--success)', flexShrink: 0 }} />
          )}
          {coordPaused && (
            <span style={{ width: 8, height: 8, borderRadius: '50%', background: 'var(--warning)', flexShrink: 0 }} />
          )}
          {!coordRunning && !coordPaused && (
            <span style={{ width: 8, height: 8, borderRadius: '50%', background: 'var(--text-muted)', flexShrink: 0 }} />
          )}
          <h1 style={{ fontSize: '16px', fontWeight: 600, color: 'var(--text-primary)', letterSpacing: '-0.01em', margin: 0 }}>
            {coordLabel}
          </h1>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, fontFamily: 'var(--font-mono)', fontSize: '11px', color: 'var(--text-muted)' }}>
            {status && (
              <>
                <span>{status.todo} todo</span>
                <span style={{ color: 'var(--border)' }}>·</span>
                <span style={{ color: (status.active > 0) ? 'var(--accent)' : undefined }}>{status.active} active</span>
                <span style={{ color: 'var(--border)' }}>·</span>
                <span style={{ color: status.blocked > 0 ? 'var(--warning)' : undefined }}>{status.blocked} blocked</span>
                <span style={{ color: 'var(--border)' }}>·</span>
                <span style={{ color: 'var(--success)' }}>{status.merged} merged</span>
              </>
            )}
            <span style={{ color: 'var(--border)' }}>·</span>
            <span>{fmtTime(elapsed)}</span>
          </div>
        </div>

        {/* Right: header actions (compact) */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          {ownerErr && (
            <span style={{ fontSize: '11px', color: 'var(--warning)', marginRight: 8 }}>{ownerErr}</span>
          )}
          {isBusy && <RefreshIcon className="h-4 w-4 animate-spin" style={{ color: 'var(--text-muted)', flexShrink: 0 }} />}
        </div>
      </div>

      {/* Failure notice */}
      {status?.failure_report && (
        <div
          style={{
            padding: '10px 14px',
            borderRadius: 'var(--radius-md)',
            background: 'oklch(0.62 0.22 25 / 0.1)',
            border: '1px solid oklch(0.62 0.22 25 / 0.35)',
            fontSize: 'var(--text-sm)',
            color: 'var(--error)',
            marginBottom: 16,
          }}
        >
          <strong>Coordinator paused:</strong> {status.failure_report.message}
          {status.failure_report.task_id && (
            <button
              type="button"
              onClick={() => navigate(`/ops/registry/tasks/${status.failure_report!.task_id}`)}
              style={{ marginLeft: 8, fontSize: '11px', color: 'var(--accent)', background: 'none', border: 'none', cursor: 'pointer', textDecoration: 'underline' }}
            >
              View task
            </button>
          )}
        </div>
      )}

      {/* ── Two-column layout ──────────────────────────────── */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 220px', gap: 16, alignItems: 'start' }}>

        {/* ── Left: workers + queue ──────────────────────── */}
        <div style={{ minWidth: 0, display: 'flex', flexDirection: 'column', gap: 20 }}>

          {/* Worker stream section */}
          <section>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10, gap: 8, flexWrap: 'wrap' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <button type="button" className={cn('filter-chip', workerFilter === 'active' && 'active')} onClick={() => setWorkerFilter('active')}>Active</button>
                <button type="button" className={cn('filter-chip', workerFilter === 'errors' && 'active')} onClick={() => setWorkerFilter('errors')}>Errors</button>
                <button type="button" className={cn('filter-chip', workerFilter === 'all' && 'active')} onClick={() => setWorkerFilter('all')}>All</button>
              </div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                {tools.length > 1 && (
                  <select
                    value={toolFilter}
                    onChange={(e) => setToolFilter(e.target.value)}
                    style={{
                      fontSize: '11px',
                      color: 'var(--text-secondary)',
                      background: 'var(--bg-card)',
                      border: '1px solid var(--border)',
                      borderRadius: 'var(--radius-sm)',
                      padding: '3px 8px',
                      outline: 'none',
                      cursor: 'pointer',
                    }}
                  >
                    {tools.map((t) => <option key={t} value={t}>{t === 'all' ? 'All tools' : t}</option>)}
                  </select>
                )}
                <span style={{ fontSize: '11px', color: 'var(--text-muted)', fontFamily: 'var(--font-mono)' }}>
                  {filteredWorkers.length} worker{filteredWorkers.length !== 1 ? 's' : ''}
                </span>
              </div>
            </div>

            {filteredWorkers.length === 0 ? (
              <div
                style={{
                  padding: '32px 16px',
                  textAlign: 'center',
                  borderRadius: 'var(--radius-lg)',
                  border: '1px dashed var(--border)',
                  color: 'var(--text-muted)',
                  fontSize: 'var(--text-sm)',
                }}
              >
                {workerFilter === 'active' ? 'No active workers.' : 'No workers match the current filter.'}
              </div>
            ) : (
              <div
                style={{
                  display: 'grid',
                  gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))',
                  gap: 10,
                }}
              >
                {filteredWorkers.map((wt, i) => (
                  <WorkerTile
                    key={wt.id}
                    worktree={wt}
                    task={worktreeTaskMap.get(wt.id)}
                    index={i}
                  />
                ))}
              </div>
            )}
          </section>

          {/* Task queue section */}
          <section>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 10 }}>
              <button type="button" className={cn('filter-chip', taskFilter === 'all' && 'active')} onClick={() => setTaskFilter('all')}>All tasks</button>
              <button type="button" className={cn('filter-chip', taskFilter === 'active' && 'active')} onClick={() => setTaskFilter('active')}>In progress</button>
              <button type="button" className={cn('filter-chip', taskFilter === 'queue' && 'active')} onClick={() => setTaskFilter('queue')}>Queue</button>
            </div>

            {sortedTasks.length === 0 ? (
              <p style={{ fontSize: 'var(--text-sm)', color: 'var(--text-muted)', padding: '16px 0' }}>No tasks match.</p>
            ) : (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
                {sortedTasks.map((t) => (
                  <TaskRow key={t.id} task={t} isFeatured={t.state === 'in_progress'} />
                ))}
                {tasks.length > 30 && taskFilter === 'all' && (
                  <button
                    type="button"
                    onClick={() => navigate('/ops/registry')}
                    style={{ fontSize: '11px', color: 'var(--accent)', background: 'none', border: 'none', cursor: 'pointer', padding: '8px 12px', textAlign: 'left' }}
                  >
                    View all {tasks.length} tasks in registry →
                  </button>
                )}
              </div>
            )}
          </section>
        </div>

        {/* ── Right: sidebar ─────────────────────────────── */}
        <aside
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: 0,
            background: 'var(--bg-card)',
            border: '1px solid var(--border)',
            borderRadius: 'var(--radius-lg)',
            padding: '14px 12px',
            position: 'sticky',
            top: 20,
          }}
        >
          {/* Primary actions */}
          <SectionLabel>Actions</SectionLabel>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 5 }}>
            <ActionBtn label="Run coordinator" variant="primary" onClick={() => handleAction('run')} disabled={isBusy || !isOwner} title={viewerHint} loading={pendingAction === 'run'} />
            <ActionBtn label="Stop" variant="danger" onClick={() => setStopConfirm(true)} disabled={isBusy || !isOwner} title={viewerHint} />
            <ActionBtn label="Resume" onClick={() => handleAction('resume')} disabled={isBusy || !isOwner} title={viewerHint} loading={pendingAction === 'resume'} />
          </div>

          <div className="divider" />

          {/* Secondary actions */}
          <SectionLabel>Control plane</SectionLabel>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
            {(['dispatch', 'advance', 'reconcile', 'sync', 'cleanup'] as ApiCoordinatorAction[]).map((a) => (
              <ActionBtn key={a} label={a.charAt(0).toUpperCase() + a.slice(1)} variant="ghost" onClick={() => handleAction(a)} disabled={isBusy || !isOwner} title={viewerHint} loading={pendingAction === a} />
            ))}
          </div>

          <div className="divider" />

          {/* Queue stats */}
          <SectionLabel>Queue</SectionLabel>
          {status ? (
            <div style={{ display: 'flex', flexDirection: 'column' }}>
              <StatRow label="Todo" value={status.todo} />
              <StatRow label="Active" value={status.active} tone="active" />
              <StatRow label="Blocked" value={status.blocked} tone={status.blocked > 0 ? 'blocked' : undefined} />
              <StatRow label="Merged" value={status.merged} tone="merged" />
              <StatRow label="Total" value={status.total} />
            </div>
          ) : (
            <p style={{ fontSize: '11px', color: 'var(--text-muted)' }}>Connecting…</p>
          )}

          {status?.effective_max_parallel != null && (
            <>
              <div className="divider" />
              <SectionLabel>Parallelism</SectionLabel>
              <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '11px', color: 'var(--text-muted)', marginBottom: 6 }}>
                <span>Workers</span>
                <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--text-primary)' }}>
                  {status.active} / {status.effective_max_parallel}
                </span>
              </div>
              <div style={{ height: 4, borderRadius: 2, background: 'var(--bg-elevated)', overflow: 'hidden' }}>
                <div
                  style={{
                    height: '100%',
                    borderRadius: 2,
                    background: 'var(--accent)',
                    width: `${Math.min(100, (status.active / status.effective_max_parallel) * 100)}%`,
                    transition: 'width 400ms ease',
                  }}
                />
              </div>
            </>
          )}

          {/* Tool cooldowns */}
          {(status?.throttled_tools?.length ?? 0) > 0 && (
            <>
              <div className="divider" />
              <SectionLabel>Throttled tools</SectionLabel>
              <ToolCooldownPanel />
            </>
          )}
        </aside>
      </div>

      {/* Emergency stop confirm */}
      <ConfirmDialog
        open={stopConfirm}
        onOpenChange={setStopConfirm}
        title="Stop coordinator"
        description="This requests the coordinator to halt. Performers in flight will finish their current step. Proceed?"
        confirmLabel="Stop coordinator"
        cancelLabel="Cancel"
        onConfirm={() => { setStopConfirm(false); void handleAction('stop'); }}
        intent="danger"
      />
    </>
  );
};

export default Console;
