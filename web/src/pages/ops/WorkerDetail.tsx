import React, { useEffect, useState, useRef, useCallback } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { getWorktrees, buildUrl } from '../../api/client';
import type { ApiWorktree } from '../../api/models';
import { resolveApiBaseUrl } from '../../api/config';
import { useEventSource } from '../../hooks/useEventSource';
import { StatusBadge } from '../../components/StatusBadge';
import { TerminalDrawer, type TerminalTarget } from '../../components/TerminalDrawer';

const MAX_LOG_LINES = 1000;

const WorkerDetail: React.FC = () => {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [worktree, setWorktree] = useState<ApiWorktree | null>(null);
  const [activeTab, setActiveTab] = useState<'logs' | 'events' | 'artifacts'>('logs');
  const [terminalOpen, setTerminalOpen] = useState(false);
  const [terminalTarget, setTerminalTarget] = useState<TerminalTarget | null>(null);

  // Logs state
  const [logs, setLogs] = useState<string[]>([]);
  const [paused, setPaused] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const logsEndRef = useRef<HTMLDivElement>(null);

  // Events
  const { events } = useEventSource('/events', { maxEvents: 500 });
  const worktreeEvents = events.filter(e => e.payload.source === id || e.payload.worktree_id === id || e.payload.task_id === id); // Best effort filtering

  useEffect(() => {
    if (id) {
      getWorktrees().then(wts => {
        const wt = wts.find(w => w.id === id);
        if (wt) setWorktree(wt);
      }).catch(console.error);
    }
  }, [id]);

  // Log Stream
  useEffect(() => {
    if (!id) return;
    let source: EventSource | null = null;
    let active = true;

    const connect = () => {
      if (!active) return;
      const path = `/worktrees/${encodeURIComponent(id)}/logs`;
      const baseUrl = resolveApiBaseUrl(undefined);
      const url = new URL(buildUrl(path, baseUrl), baseUrl ?? window.location.origin);
      
      source = new EventSource(url.toString());

      source.addEventListener('log_line', (event: MessageEvent<string>) => {
        if (!active) return;
        try {
          const payload = JSON.parse(event.data);
          const message = typeof payload.message === 'string' ? payload.message : (payload.content || JSON.stringify(payload));
          
          if (!paused) {
            setLogs((prev) => {
              const next = [...prev, message];
              if (next.length > MAX_LOG_LINES) return next.slice(next.length - MAX_LOG_LINES);
              return next;
            });
          }
        } catch {
          // ignore
        }
      });
    };

    connect();
    return () => {
      active = false;
      if (source) source.close();
    };
  }, [id, paused]);

  // Auto-scroll logs
  useEffect(() => {
    if (!paused && logsEndRef.current && activeTab === 'logs') {
      logsEndRef.current.scrollIntoView({ behavior: 'smooth' });
    }
  }, [logs, paused, activeTab]);

  const handleCopyDiagnostics = useCallback(() => {
    const lastNLogs = logs.slice(-100).join('\n');
    const keyEvents = worktreeEvents.slice(0, 10).map(e => `${e.receivedAt}: ${e.payload.type}`).join('\n');
    const bundle = `--- Worker Diagnostics: ${id} ---\n\nLast 100 Logs:\n${lastNLogs}\n\nRecent Events:\n${keyEvents}`;
    navigator.clipboard.writeText(bundle).catch(console.error);
  }, [id, logs, worktreeEvents]);

  const handleOpenTerminal = useCallback(() => {
    setTerminalTarget({
      terminalType: 'worktree',
      worktreeId: id ?? undefined,
      label: `Worktree: ${worktree?.slug || id}`,
    });
    setTerminalOpen(true);
  }, [id, worktree?.slug]);

  const filteredLogs = logs.filter(l => l.toLowerCase().includes(searchQuery.toLowerCase()));

  if (!id) return <div className="p-8">No Worker ID provided</div>;

  return (
    <div className="flex flex-col h-screen bg-[var(--bg-primary)]">
      {/* Header */}
      <header className="flex-none flex items-center justify-between px-6 py-4 border-b border-white/10 bg-[var(--bg-secondary)]">
        <div className="flex items-center space-x-4">
          <button onClick={() => navigate('/ops/live')} className="text-[var(--text-muted)] hover:text-white transition-colors">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><line x1="19" y1="12" x2="5" y2="12"></line><polyline points="12 19 5 12 12 5"></polyline></svg>
          </button>
          <div className="flex flex-col">
            <div className="flex items-center space-x-3">
              <h1 className="text-xl font-semibold text-white">Worker: {id}</h1>
              {worktree && <StatusBadge status={worktree.status || 'unknown'} tone={worktree.status === 'failed' ? 'failed' : 'active'} />}
            </div>
            {worktree && <p className="text-sm text-[var(--text-muted)] mt-1">Path: {worktree.path} • Tool: {worktree.tool}</p>}
          </div>
        </div>
        
        <div className="flex items-center space-x-3">
          <button onClick={() => console.log('Stop worker')} className="flex items-center space-x-2 px-3 py-1.5 rounded bg-red-500/10 text-red-400 hover:bg-red-500/20 transition-colors border border-red-500/20 text-sm font-medium">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="10"></circle><line x1="15" y1="9" x2="9" y2="15"></line><line x1="9" y1="9" x2="15" y2="15"></line></svg>
            <span>Stop</span>
          </button>
          <button onClick={() => console.log('Restart phase')} className="flex items-center space-x-2 px-3 py-1.5 rounded bg-white/5 text-white hover:bg-white/10 transition-colors border border-white/10 text-sm font-medium">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="23 4 23 10 17 10"></polyline><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"></path></svg>
            <span>Restart Phase</span>
          </button>
          <button onClick={handleOpenTerminal} className="flex items-center space-x-2 px-3 py-1.5 rounded bg-white/5 text-white hover:bg-white/10 transition-colors border border-white/10 text-sm font-medium">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="4 17 10 11 4 5"></polyline><line x1="12" y1="19" x2="20" y2="19"></line></svg>
            <span>Terminal</span>
          </button>
          <button onClick={handleCopyDiagnostics} className="flex items-center space-x-2 px-3 py-1.5 rounded bg-white/5 text-white hover:bg-white/10 transition-colors border border-white/10 text-sm font-medium">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>
            <span>Diagnostics</span>
          </button>
        </div>
      </header>

      {/* Tabs */}
      <div className="flex-none px-6 border-b border-white/10 bg-[var(--bg-secondary)] flex space-x-6">
        {(['logs', 'events', 'artifacts'] as const).map(tab => (
          <button
            key={tab}
            onClick={() => setActiveTab(tab)}
            className={`py-3 text-sm font-medium border-b-2 transition-colors ${
              activeTab === tab ? 'border-[var(--accent)] text-white' : 'border-transparent text-[var(--text-muted)] hover:text-white'
            }`}
          >
            {tab.charAt(0).toUpperCase() + tab.slice(1)}
          </button>
        ))}
      </div>

      {/* Content */}
      <main className="flex-1 overflow-hidden relative">
        {activeTab === 'logs' && (
          <div className="absolute inset-0 flex flex-col">
            <div className="flex-none p-4 border-b border-white/10 bg-[var(--bg-primary)] flex items-center justify-between">
              <div className="relative">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="absolute left-3 top-1/2 -translate-y-1/2 text-[var(--text-muted)]"><circle cx="11" cy="11" r="8"></circle><line x1="21" y1="21" x2="16.65" y2="16.65"></line></svg>
                <input
                  type="text"
                  placeholder="Search logs..."
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="pl-9 pr-4 py-1.5 bg-black/20 border border-white/10 rounded text-sm text-white placeholder-[var(--text-muted)] focus:outline-none focus:border-[var(--accent)]"
                />
              </div>
              <button
                onClick={() => setPaused(!paused)}
                className={`px-3 py-1.5 rounded text-sm font-medium border transition-colors ${
                  paused ? 'bg-yellow-500/20 text-yellow-500 border-yellow-500/30' : 'bg-white/5 text-[var(--text-secondary)] border-white/10 hover:bg-white/10'
                }`}
              >
                {paused ? 'Resume Scroll' : 'Pause Scroll'}
              </button>
            </div>
            <div className="flex-1 overflow-auto p-4 font-mono text-sm bg-black/40">
              {filteredLogs.map((log, i) => (
                <div key={i} className="py-0.5 text-gray-300 break-words whitespace-pre-wrap">{log}</div>
              ))}
              <div ref={logsEndRef} />
              {filteredLogs.length === 0 && <div className="text-[var(--text-muted)]">No logs available.</div>}
            </div>
          </div>
        )}

        {activeTab === 'events' && (
          <div className="absolute inset-0 overflow-auto p-6">
            <div className="max-w-4xl mx-auto">
              <h2 className="text-lg font-semibold text-white mb-6">Events Timeline</h2>
              <div className="space-y-4 relative before:absolute before:inset-0 before:ml-5 before:-translate-x-px md:before:mx-auto md:before:translate-x-0 before:h-full before:w-0.5 before:bg-gradient-to-b before:from-transparent before:via-white/10 before:to-transparent">
                {worktreeEvents.map((event, idx) => (
                  <div key={idx} className="relative flex items-center justify-between md:justify-normal md:odd:flex-row-reverse group is-active">
                    <div className="flex items-center justify-center w-10 h-10 rounded-full border border-white/10 bg-[var(--bg-secondary)] text-[var(--text-secondary)] shadow shrink-0 md:order-1 md:group-odd:-translate-x-1/2 md:group-even:translate-x-1/2">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="10"></circle><polyline points="12 6 12 12 16 14"></polyline></svg>
                    </div>
                    <div className="w-[calc(100%-4rem)] md:w-[calc(50%-2.5rem)] p-4 rounded border border-white/10 bg-white/5 shadow">
                      <div className="flex items-center justify-between space-x-2 mb-1">
                        <div className="font-bold text-white text-sm">{event.payload.type}</div>
                        <time className="text-xs text-[var(--text-muted)]">{new Date(event.receivedAt).toLocaleTimeString()}</time>
                      </div>
                      <div className="text-sm text-[var(--text-secondary)] break-words">
                        {JSON.stringify(event.payload, null, 2)}
                      </div>
                    </div>
                  </div>
                ))}
                {worktreeEvents.length === 0 && (
                  <div className="text-center text-[var(--text-muted)] py-10 relative z-10">No recent events found for this worker.</div>
                )}
              </div>
            </div>
          </div>
        )}

        {activeTab === 'artifacts' && (
          <div className="absolute inset-0 overflow-auto p-6">
            <div className="max-w-4xl mx-auto">
              <h2 className="text-lg font-semibold text-white mb-6">Worktree Artifacts</h2>
              <div className="bg-white/5 border border-white/10 rounded-lg overflow-hidden">
                <div className="p-4 border-b border-white/10 bg-white/5 flex items-center">
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-[var(--text-muted)] mr-3"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path></svg>
                  <span className="font-mono text-sm text-[var(--text-secondary)]">{worktree?.path || 'Loading path...'}</span>
                </div>
                <div className="divide-y divide-white/5">
                  <div className="p-4 hover:bg-white/5 transition-colors flex items-center justify-between">
                    <div className="flex items-center">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-[var(--text-muted)] mr-3"><path d="M14.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5L14.5 2z"></path><polyline points="14 2 14 8 20 8"></polyline></svg>
                      <span className="text-sm text-white">worktree.prd.json</span>
                    </div>
                    <span className="text-xs text-[var(--text-muted)]">Configuration</span>
                  </div>
                  <div className="p-4 hover:bg-white/5 transition-colors flex items-center justify-between">
                    <div className="flex items-center">
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-[var(--text-muted)] mr-3"><path d="M14.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5L14.5 2z"></path><polyline points="14 2 14 8 20 8"></polyline></svg>
                      <span className="text-sm text-white">Generated Files</span>
                    </div>
                    <span className="text-xs text-[var(--text-muted)]">Multiple</span>
                  </div>
                </div>
                <div className="p-4 bg-black/20 text-center text-sm text-[var(--text-muted)]">
                  Artifacts view is currently limited to basic listing. Full diff view coming soon.
                </div>
              </div>
            </div>
          </div>
        )}
      </main>

      <TerminalDrawer
        open={terminalOpen}
        onOpenChange={setTerminalOpen}
        defaultTarget={terminalTarget}
        projectRootLabel="Project Root"
      />
    </div>
  );
};

export default WorkerDetail;
