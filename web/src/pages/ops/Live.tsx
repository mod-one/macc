import React, { useEffect, useState, useMemo, useCallback, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { getWorktrees } from '../../api/client';
import type { ApiWorktree } from '../../api/models';
import { StreamTile } from '../../components/StreamTile';
import { buildUrl } from '../../api/client';
import { resolveApiBaseUrl } from '../../api/config';

// Spec requires bounded buffer per stream (truncate oldest)
const MAX_LOG_LINES = 500;

function isWorktreeActive(status?: string | null): boolean {
  if (!status) return false;
  const s = status.toLowerCase();
  return s === 'running' || s === 'active' || s === 'in_progress';
}

function isWorktreeError(status?: string | null): boolean {
  if (!status) return false;
  const s = status.toLowerCase();
  return s === 'failed' || s === 'error';
}

function useWorktreeLogStream(worktreeId: string, paused: boolean) {
  const [logs, setLogs] = useState<string[]>([]);
  const [connectionState, setConnectionState] = useState<'connecting' | 'open' | 'closed' | 'error'>('connecting');
  const pausedRef = useRef(paused);

  useEffect(() => {
    pausedRef.current = paused;
  }, [paused]);

  useEffect(() => {
    let source: EventSource | null = null;
    let active = true;

    const connect = () => {
      if (!active) return;

      const path = `/worktrees/${encodeURIComponent(worktreeId)}/logs`;
      const baseUrl = resolveApiBaseUrl(undefined);
      const url = new URL(buildUrl(path, baseUrl), baseUrl ?? window.location.origin);
      
      source = new EventSource(url.toString());
      setConnectionState('connecting');

      source.addEventListener('open', () => {
        if (active) setConnectionState('open');
      });

      source.addEventListener('error', () => {
        if (active) {
          setConnectionState('error');
        }
      });

      source.addEventListener('log_line', (event: MessageEvent<string>) => {
        if (!active) return;
        try {
          const payload = JSON.parse(event.data);
          const message = typeof payload.message === 'string' ? payload.message : (payload.content || JSON.stringify(payload));
          
          if (!pausedRef.current) {
            setLogs((prev) => {
              const next = [...prev, message];
              if (next.length > MAX_LOG_LINES) {
                return next.slice(next.length - MAX_LOG_LINES);
              }
              return next;
            });
          }
        } catch {
          // ignore parsing error for single line
        }
      });
    };

    connect();

    return () => {
      active = false;
      if (source) {
        source.close();
      }
    };
  }, [worktreeId]);

  return { logs, connectionState };
}

const StreamTileWrapper: React.FC<{ worktree: ApiWorktree; navigate: ReturnType<typeof useNavigate> }> = ({ worktree, navigate }) => {
  const [paused, setPaused] = useState(false);
  const { logs, connectionState } = useWorktreeLogStream(worktree.id, paused);

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(logs.join('\n')).catch(console.error);
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

  const getStatusText = () => {
    if (worktree.status === 'failed') return 'Failed';
    if (connectionState === 'open') return 'Connected';
    if (connectionState === 'connecting' || connectionState === 'error') return 'Reconnecting...';
    return 'Disconnected';
  };

  const getStatusTone = () => {
    if (worktree.status === 'failed') return 'failed';
    if (connectionState === 'open') return 'active';
    if (connectionState === 'connecting' || connectionState === 'error') return 'todo';
    return 'paused';
  };

  return (
    <StreamTile
      title={worktree.id}
      tool={worktree.tool || 'unknown'}
      liveLogTail={logs}
      status={getStatusText()}
      statusTone={getStatusTone()}
      paused={paused}
      onPauseToggle={() => setPaused(!paused)}
      onCopy={handleCopy}
      onDownload={handleDownload}
      onClick={() => navigate(`/ops/worker/${encodeURIComponent(worktree.id)}`)}
    />
  );
};

const Live: React.FC = () => {
  const [worktrees, setWorktrees] = useState<ApiWorktree[]>([]);
  const [activeOnly, setActiveOnly] = useState(true);
  const [selectedTool, setSelectedTool] = useState('all');
  const [errorsOnly, setErrorsOnly] = useState(false);
  const navigate = useNavigate();

  useEffect(() => {
    const fetchWT = async () => {
      try {
        const wts = await getWorktrees();
        setWorktrees(wts);
      } catch (err) {
        console.error('Failed to fetch worktrees', err);
      }
    };
    fetchWT();
    const interval = setInterval(fetchWT, 5000);
    return () => clearInterval(interval);
  }, []);

  const tools = useMemo(() => {
    const ts = new Set(worktrees.map((w) => w.tool).filter(Boolean) as string[]);
    return ['all', ...Array.from(ts).sort()];
  }, [worktrees]);

  const filteredWorktrees = useMemo(() => {
    return worktrees.filter(wt => {
      if (activeOnly && !isWorktreeActive(wt.status)) return false;
      if (selectedTool !== 'all' && wt.tool !== selectedTool) return false;
      if (errorsOnly && !isWorktreeError(wt.status)) return false;
      return true;
    });
  }, [worktrees, activeOnly, selectedTool, errorsOnly]);

  return (
    <div className="flex flex-col gap-6">
      <header className="rounded-[2rem] border border-slate-200 bg-white p-6 shadow-sm">
        <h1 className="text-5xl font-semibold tracking-tight text-slate-950">Live Wall</h1>
        <p className="mt-3 text-base text-slate-600">
          Monitor multiple worktree stream logs concurrently. Supports {">"} 20 concurrent tiles.
        </p>

        {/* Filter Bar */}
        <div className="mt-6 flex flex-wrap items-center gap-4 border-t border-slate-100 pt-6">
          <label className="flex items-center gap-2 text-sm font-medium text-slate-700 cursor-pointer">
            <input 
              type="checkbox" 
              checked={activeOnly} 
              onChange={(e) => setActiveOnly(e.target.checked)} 
              className="rounded border-slate-300 text-sky-600 focus:ring-sky-500"
            />
            Active Only
          </label>

          <label className="flex items-center gap-2 text-sm font-medium text-slate-700 cursor-pointer">
            <input 
              type="checkbox" 
              checked={errorsOnly} 
              onChange={(e) => setErrorsOnly(e.target.checked)} 
              className="rounded border-slate-300 text-sky-600 focus:ring-sky-500"
            />
            Errors Only
          </label>

          <div className="flex items-center gap-2 ml-auto">
            <span className="text-sm font-medium text-slate-700">Tool:</span>
            <select
              value={selectedTool}
              onChange={(e) => setSelectedTool(e.target.value)}
              className="rounded-lg border-slate-300 bg-slate-50 text-sm focus:border-sky-500 focus:ring-sky-500 outline-none p-2"
            >
              {tools.map(t => (
                <option key={t} value={t}>{t === 'all' ? 'All Tools' : t}</option>
              ))}
            </select>
          </div>
        </div>
      </header>

      <section>
        {filteredWorktrees.length === 0 ? (
          <div className="rounded-2xl border border-dashed border-slate-300 bg-slate-50 py-16 text-center">
            <p className="text-slate-500">No worktrees match the current filters.</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5">
            {filteredWorktrees.map((wt) => (
              <StreamTileWrapper key={wt.id} worktree={wt} navigate={navigate} />
            ))}
          </div>
        )}
      </section>
    </div>
  );
};

export default Live;
