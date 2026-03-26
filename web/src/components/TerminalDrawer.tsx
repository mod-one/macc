import React from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import { FitAddon } from 'xterm-addon-fit';
import { Terminal } from 'xterm';
import 'xterm/css/xterm.css';
import { buildWebSocketUrl, createTerminalSession } from '../api/client';
import type { ApiTerminalCreateRequest, ApiTerminalType } from '../api/models';
import { Button } from './Button';
import { XCircleIcon, RefreshIcon, PlusIcon, FolderIcon, ClockIcon, AlertTriangleIcon } from './icons';
import { cn } from './styles';

type TerminalTarget = {
  terminalType: ApiTerminalType;
  label: string;
  worktreeId?: string;
};

type TerminalSessionStatus = 'connecting' | 'connected' | 'disconnected' | 'error';

interface TerminalSessionRecord {
  id: string;
  target: TerminalTarget;
  backendSessionId: string | null;
  path: string | null;
  status: TerminalSessionStatus;
  error: string | null;
  createdAt: number;
}

interface TerminalDrawerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  defaultTarget?: TerminalTarget | null;
  projectRootLabel?: string;
}

interface SessionPaneProps {
  session: TerminalSessionRecord;
  active: boolean;
  fontSize: number;
  onStatusChange: (id: string, status: TerminalSessionStatus, error?: string | null) => void;
  onReconnect: (session: TerminalSessionRecord) => void;
  onCloseSession: (sessionId: string) => void;
}

const MIN_HEIGHT = 280;
const MAX_HEIGHT_RATIO = 0.9;
const DEFAULT_HEIGHT_RATIO = 0.62;
const MAX_SESSIONS = 5;

function makeSessionId() {
  return `term-local-${Math.random().toString(36).slice(2, 10)}`;
}

function getTargetKey(target: TerminalTarget) {
  return target.terminalType === 'project' ? 'project' : `worktree:${target.worktreeId ?? ''}`;
}

function getTargetLabel(target: TerminalTarget) {
  return target.label.trim() || (target.terminalType === 'project' ? 'Project Root' : 'Worktree');
}

function getTerminalCreateRequest(target: TerminalTarget): ApiTerminalCreateRequest {
  return target.terminalType === 'project'
    ? { terminalType: 'project' }
    : { terminalType: 'worktree', worktreeId: target.worktreeId ?? null };
}

function SessionPane({
  session,
  active,
  fontSize,
  onStatusChange,
  onReconnect,
  onCloseSession,
}: SessionPaneProps) {
  const hostRef = React.useRef<HTMLDivElement | null>(null);
  const terminalRef = React.useRef<Terminal | null>(null);
  const fitAddonRef = React.useRef<FitAddon | null>(null);
  const socketRef = React.useRef<WebSocket | null>(null);
  const decoderRef = React.useRef(new TextDecoder());
  const closeReasonRef = React.useRef<'tab-close' | 'drawer-close' | 'reconnect' | null>(null);
  const [resizeTick, setResizeTick] = React.useState(0);

  React.useEffect(() => {
    if (!session.backendSessionId || !hostRef.current) {
      return undefined;
    }

    const host = hostRef.current;
    const terminal = new Terminal({
      cursorBlink: true,
      fontFamily: 'var(--font-mono)',
      fontSize,
      lineHeight: 1.15,
      scrollback: 2000,
      theme: {
        background: '#0b0f19',
        foreground: '#e5e7eb',
        cursor: '#f8fafc',
        selectionBackground: 'rgba(59, 130, 246, 0.35)',
      },
      allowTransparency: true,
    });
    const fitAddon = new FitAddon();

    terminal.loadAddon(fitAddon);
    terminal.open(host);
    terminal.write(`\u001b[2J\u001b[HConnecting to ${session.target.label}...\r\n`);
    terminalRef.current = terminal;
    fitAddonRef.current = fitAddon;

    const ResizeObserverImpl = window.ResizeObserver;
    const resizeObserver = ResizeObserverImpl
      ? new ResizeObserverImpl(() => {
          setResizeTick((value) => value + 1);
        })
      : null;
    resizeObserver?.observe(host);

    const ws = new WebSocket(buildWebSocketUrl(`/terminal/${session.backendSessionId}`));
    ws.binaryType = 'arraybuffer';
    socketRef.current = ws;

    let disposed = false;

    const writeChunk = (chunk: ArrayBuffer | Uint8Array | string) => {
      if (!terminalRef.current) return;
      if (typeof chunk === 'string') {
        terminalRef.current.write(chunk);
        return;
      }
      const bytes = chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk);
      terminalRef.current.write(decoderRef.current.decode(bytes, { stream: true }));
    };

    const fit = () => {
      try {
        fitAddon.fit();
      } catch {
        // Ignore transient layout failures while the drawer animates.
      }
    };

    const disconnect = (status: TerminalSessionStatus, error?: string | null) => {
      if (!disposed) {
        onStatusChange(session.id, status, error ?? null);
      }
    };

    ws.addEventListener('open', () => {
      if (disposed) return;
      disconnect('connected', null);
      fit();
      terminal.focus();
    });

    ws.addEventListener('message', async (event) => {
      if (disposed) return;
      if (typeof event.data === 'string') {
        writeChunk(event.data);
        return;
      }
      if (event.data instanceof Blob) {
        const buffer = await event.data.arrayBuffer();
        if (disposed) return;
        writeChunk(buffer);
        return;
      }
      if (event.data instanceof ArrayBuffer) {
        writeChunk(event.data);
      }
    });

    ws.addEventListener('close', () => {
      if (disposed) return;
      const nextStatus: TerminalSessionStatus = closeReasonRef.current ? 'disconnected' : 'disconnected';
      disconnect(nextStatus, null);
      closeReasonRef.current = null;
    });

    ws.addEventListener('error', () => {
      if (disposed) return;
      disconnect('error', 'WebSocket connection failed.');
    });

    const disposableData = terminal.onData((data) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(data);
      }
    });

    const handleFit = () => {
      fit();
    };

    window.addEventListener('resize', handleFit);

    fit();

    return () => {
      disposed = true;
      window.removeEventListener('resize', handleFit);
      resizeObserver?.disconnect();
      disposableData.dispose();
      if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) {
        ws.close();
      }
      socketRef.current = null;
      terminal.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
    };
  }, [fontSize, onStatusChange, session.backendSessionId, session.id, session.target.label]);

  React.useEffect(() => {
    if (!terminalRef.current) {
      return;
    }
    terminalRef.current.options.fontSize = fontSize;
    if (active) {
      try {
        fitAddonRef.current?.fit();
      } catch {
        // Ignore transient layout failures while the drawer animates.
      }
    }
  }, [active, fontSize]);

  React.useEffect(() => {
    if (active && terminalRef.current && fitAddonRef.current) {
      try {
        fitAddonRef.current.fit();
        terminalRef.current.focus();
      } catch {
        // Ignore layout timing issues during drawer animation.
      }
    }
  }, [active, resizeTick, fontSize]);

  const handleReconnect = () => {
    closeReasonRef.current = 'reconnect';
    onReconnect(session);
  };

  const handleClose = () => {
    closeReasonRef.current = 'tab-close';
    socketRef.current?.close();
    onCloseSession(session.id);
  };

  return (
    <section className={cn('h-full min-h-0', active ? 'block' : 'hidden')}>
      <div className="flex h-full min-h-0 flex-col rounded-2xl border border-[var(--border)] bg-[#07111f] shadow-inner shadow-black/40">
        <div className="flex items-center justify-between border-b border-white/10 px-4 py-3 text-sm text-[var(--text-secondary)]">
          <div className="flex items-center gap-3">
            <span className="font-semibold text-[var(--text-primary)]">{session.target.label}</span>
            <span
              className={cn(
                'rounded-full px-2 py-0.5 text-[11px] font-semibold uppercase tracking-wider',
                session.status === 'connected'
                  ? 'bg-emerald-500/15 text-emerald-300'
                  : session.status === 'connecting'
                    ? 'bg-sky-500/15 text-sky-300'
                    : session.status === 'error'
                      ? 'bg-rose-500/15 text-rose-300'
                      : 'bg-amber-500/15 text-amber-300',
              )}
            >
              {session.status}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <button
              className="inline-flex h-8 items-center gap-2 rounded-md border border-white/10 bg-white/5 px-3 text-xs font-medium transition-colors hover:bg-white/10"
              onClick={handleReconnect}
              type="button"
            >
              <RefreshIcon className="h-3.5 w-3.5" />
              Reconnect
            </button>
            <button
              aria-label={`Close terminal ${session.target.label}`}
              className="inline-flex h-8 w-8 items-center justify-center rounded-md border border-white/10 bg-white/5 transition-colors hover:bg-white/10"
              onClick={handleClose}
              type="button"
            >
              <XCircleIcon className="h-4 w-4" />
            </button>
          </div>
        </div>
        <div className="relative min-h-0 flex-1">
          <div ref={hostRef} className="absolute inset-0" />
          {session.status !== 'connected' && (
            <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-[#07111f]/70">
              <div className="pointer-events-auto max-w-sm rounded-2xl border border-white/10 bg-[var(--bg-card)] px-4 py-3 text-sm text-[var(--text-secondary)] shadow-xl">
                {session.status === 'connecting' && <p>Opening terminal session...</p>}
                {session.status === 'disconnected' && (
                  <p>Session disconnected. Reconnect to create a new terminal session.</p>
                )}
                {session.status === 'error' && (
                  <p>{session.error || 'Terminal connection failed.'}</p>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    </section>
  );
}

export function TerminalDrawer({
  open,
  onOpenChange,
  defaultTarget,
  projectRootLabel = 'Project Root',
}: TerminalDrawerProps) {
  const [sessions, setSessions] = React.useState<TerminalSessionRecord[]>([]);
  const [activeSessionId, setActiveSessionId] = React.useState<string | null>(null);
  const [fontSize, setFontSize] = React.useState(14);
  const [drawerHeight, setDrawerHeight] = React.useState(() =>
    Math.round((typeof window !== 'undefined' ? window.innerHeight : 900) * DEFAULT_HEIGHT_RATIO),
  );
  const [statusMessage, setStatusMessage] = React.useState<string | null>(null);
  const resizingRef = React.useRef(false);
  const pendingCreateRef = React.useRef<string | null>(null);

  const targetFromDefault = React.useMemo<TerminalTarget | null>(() => {
    if (!defaultTarget) {
      return null;
    }
    return defaultTarget.terminalType === 'project'
      ? { terminalType: 'project', label: defaultTarget.label || projectRootLabel }
      : {
          terminalType: 'worktree',
          worktreeId: defaultTarget.worktreeId,
          label: defaultTarget.label,
        };
  }, [defaultTarget, projectRootLabel]);

  const updateSession = React.useCallback(
    (sessionId: string, patch: Partial<TerminalSessionRecord>) => {
      setSessions((current) =>
        current.map((session) => (session.id === sessionId ? { ...session, ...patch } : session)),
      );
    },
    [],
  );

  const ensureTerminal = React.useCallback(
    async (target: TerminalTarget) => {
      const targetKey = getTargetKey(target);
      const existing = sessions.find((session) => getTargetKey(session.target) === targetKey);
      if (existing) {
        setActiveSessionId(existing.id);
        if (existing.status !== 'connected' && existing.status !== 'connecting') {
          pendingCreateRef.current = existing.id;
        }
        return existing.id;
      }

      if (sessions.length >= MAX_SESSIONS) {
        setStatusMessage(`Terminal limit reached. Close a tab before opening another session.`);
        return null;
      }

      const localId = makeSessionId();
      const record: TerminalSessionRecord = {
        id: localId,
        target,
        backendSessionId: null,
        path: null,
        status: 'connecting',
        error: null,
        createdAt: Date.now(),
      };

      setSessions((current) => [...current, record]);
      setActiveSessionId(localId);
      setStatusMessage(null);
      pendingCreateRef.current = localId;

      try {
        const created = await createTerminalSession(getTerminalCreateRequest(target));
        updateSession(localId, {
          backendSessionId: created.sessionId,
          path: created.path,
          status: 'connecting',
          error: null,
        });
        return localId;
      } catch (error) {
        const message = error instanceof Error ? error.message : 'Failed to create terminal session.';
        updateSession(localId, {
          status: 'error',
          error: message,
        });
        setStatusMessage(message);
        return null;
      } finally {
        if (pendingCreateRef.current === localId) {
          pendingCreateRef.current = null;
        }
      }
    },
    [sessions, updateSession],
  );

  const handleReconnect = React.useCallback(
    async (session: TerminalSessionRecord) => {
      if (session.status === 'connecting') {
        return;
      }
      updateSession(session.id, {
        status: 'connecting',
        error: null,
        backendSessionId: null,
      });
      try {
        const created = await createTerminalSession(getTerminalCreateRequest(session.target));
        updateSession(session.id, {
          backendSessionId: created.sessionId,
          path: created.path,
          status: 'connecting',
          error: null,
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : 'Failed to reconnect terminal session.';
        updateSession(session.id, {
          status: 'error',
          error: message,
        });
        setStatusMessage(message);
      }
    },
    [updateSession],
  );

  const handleStatusChange = React.useCallback(
    (sessionId: string, status: TerminalSessionStatus, error: string | null = null) => {
      updateSession(sessionId, { status, error });
    },
    [updateSession],
  );

  const handleCloseSession = React.useCallback((sessionId: string) => {
    setSessions((current) => {
      const next = current.filter((session) => session.id !== sessionId);
      if (activeSessionId === sessionId) {
        setActiveSessionId(next[0]?.id ?? null);
      }
      return next;
    });
  }, [activeSessionId]);

  React.useEffect(() => {
    if (!open) {
      setStatusMessage(null);
      setActiveSessionId(null);
      setSessions([]);
    }
  }, [open]);

  React.useEffect(() => {
    if (!open || !targetFromDefault) {
      return;
    }
    void ensureTerminal(targetFromDefault);
  }, [ensureTerminal, open, targetFromDefault]);

  React.useEffect(() => {
    const onPointerMove = (event: PointerEvent) => {
      if (!resizingRef.current) return;
      const maxHeight = Math.round(window.innerHeight * MAX_HEIGHT_RATIO);
      const nextHeight = Math.max(MIN_HEIGHT, Math.min(maxHeight, window.innerHeight - event.clientY));
      setDrawerHeight(nextHeight);
    };

    const onPointerUp = () => {
      resizingRef.current = false;
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };

    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', onPointerUp);
    return () => {
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', onPointerUp);
    };
  }, []);

  React.useEffect(() => {
    if (!open) {
      return;
    }
    const maxHeight = Math.round(window.innerHeight * MAX_HEIGHT_RATIO);
    setDrawerHeight((current) => Math.max(MIN_HEIGHT, Math.min(maxHeight, current)));
  }, [open]);

  const activeSession = sessions.find((session) => session.id === activeSessionId) ?? sessions[0] ?? null;

  const beginResize = () => {
    resizingRef.current = true;
    document.body.style.cursor = 'row-resize';
    document.body.style.userSelect = 'none';
  };

  const handleOpenProjectRoot = () => {
    void ensureTerminal({ terminalType: 'project', label: projectRootLabel });
  };

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/70 backdrop-blur-sm data-[state=open]:animate-[fadeIn_150ms_ease-out]" />
        <Dialog.Content
          className="fixed inset-x-0 bottom-0 z-50 flex flex-col border-t border-white/10 bg-[linear-gradient(180deg,rgba(7,17,31,0.98),rgba(3,7,18,0.98))] text-[var(--text-primary)] shadow-[0_-24px_80px_rgba(0,0,0,0.55)] focus:outline-none"
          style={{ height: drawerHeight }}
        >
          <div
            aria-label="Resize terminal drawer"
            className="flex cursor-row-resize items-center justify-center border-b border-white/10 px-4 py-2"
            onPointerDown={beginResize}
            role="separator"
          >
            <div className="h-1 w-20 rounded-full bg-white/20" />
          </div>
          <header className="flex items-start justify-between gap-4 border-b border-white/10 px-5 py-4">
            <div className="space-y-1">
              <Dialog.Title className="text-lg font-semibold">Terminal</Dialog.Title>
              <Dialog.Description className="text-sm text-[var(--text-secondary)]">
                PTY over WebSocket sessions for the project root and individual worktrees.
              </Dialog.Description>
              {statusMessage && <p className="text-sm text-amber-300">{statusMessage}</p>}
            </div>
            <div className="flex flex-col items-end gap-3">
              <div className="flex items-center gap-2">
                <label className="flex items-center gap-2 rounded-md border border-white/10 bg-white/5 px-3 py-2 text-xs text-[var(--text-secondary)]">
                  <ClockIcon className="h-4 w-4" />
                  Font
                  <input
                    aria-label="Terminal font size"
                    className="w-14 bg-transparent text-right font-mono text-sm text-[var(--text-primary)] outline-none"
                    max={24}
                    min={11}
                    onChange={(event) => setFontSize(Number(event.target.value))}
                    type="range"
                    value={fontSize}
                  />
                  <span className="w-6 text-right font-mono text-sm text-[var(--text-primary)]">{fontSize}</span>
                </label>
                <Button className="gap-2 h-10" onClick={handleOpenProjectRoot} type="button">
                  <PlusIcon className="h-4 w-4" />
                  Project Root
                </Button>
              </div>
              <Dialog.Close asChild>
                <button
                  aria-label="Close terminal drawer"
                  className="inline-flex h-10 w-10 items-center justify-center rounded-md border border-white/10 bg-white/5 transition-colors hover:bg-white/10"
                  type="button"
                >
                  <XCircleIcon className="h-4 w-4" />
                </button>
              </Dialog.Close>
            </div>
          </header>

          <div className="flex min-h-0 flex-1 flex-col gap-4 px-5 py-4">
            <div className="flex items-center justify-between gap-4">
              <div className="flex flex-wrap items-center gap-2">
                {sessions.map((session) => (
                  <button
                    key={session.id}
                    className={cn(
                      'inline-flex items-center gap-2 rounded-full border px-3 py-1.5 text-sm font-medium transition-colors',
                      session.id === activeSessionId
                        ? 'border-[var(--accent)] bg-[var(--accent)]/15 text-[var(--text-primary)]'
                        : 'border-white/10 bg-white/5 text-[var(--text-secondary)] hover:bg-white/10',
                    )}
                    onClick={() => setActiveSessionId(session.id)}
                    type="button"
                  >
                    <span>{getTargetLabel(session.target)}</span>
                    <span className="rounded-full bg-black/30 px-2 py-0.5 text-[10px] uppercase tracking-wider">
                      {session.status}
                    </span>
                  </button>
                ))}
                {sessions.length === 0 && (
                  <div className="inline-flex items-center gap-2 rounded-full border border-dashed border-white/10 px-3 py-1.5 text-sm text-[var(--text-muted)]">
                    <FolderIcon className="h-4 w-4" />
                    No terminal sessions yet
                  </div>
                )}
              </div>
              <div className="text-xs text-[var(--text-muted)]">
                {sessions.length}/{MAX_SESSIONS} tabs
              </div>
            </div>

            {sessions.length >= MAX_SESSIONS && (
              <div className="flex items-center gap-2 rounded-xl border border-amber-500/20 bg-amber-500/10 px-3 py-2 text-sm text-amber-200">
                <AlertTriangleIcon className="h-4 w-4" />
                Backend limit reached. Close a tab to open another session.
              </div>
            )}

            <div className="min-h-0 flex-1">
              {sessions.length === 0 ? (
                <div className="flex h-full items-center justify-center rounded-2xl border border-dashed border-white/10 bg-white/5 text-center">
                  <div className="max-w-md space-y-3 px-6 py-10">
                    <FolderIcon className="mx-auto h-12 w-12 text-[var(--text-muted)] opacity-30" />
                    <h2 className="text-lg font-semibold text-[var(--text-primary)]">Open a terminal session</h2>
                    <p className="text-sm text-[var(--text-secondary)]">
                      Start with the project root or open this drawer from a worktree context action.
                    </p>
                    <Button className="gap-2 h-10" onClick={handleOpenProjectRoot} type="button">
                      <PlusIcon className="h-4 w-4" />
                      Open Project Root
                    </Button>
                  </div>
                </div>
              ) : (
                <div className="h-full min-h-0">
                  {sessions.map((session) => (
                    <SessionPane
                      key={session.id}
                      active={session.id === activeSession?.id}
                      fontSize={fontSize}
                      onCloseSession={handleCloseSession}
                      onReconnect={handleReconnect}
                      onStatusChange={handleStatusChange}
                      session={session}
                    />
                  ))}
                </div>
              )}
            </div>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

export type { TerminalTarget };
