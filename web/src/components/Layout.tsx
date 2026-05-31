import React, { useEffect, useRef, useState } from 'react';
import { Link, Outlet, useLocation } from 'react-router-dom';
import { Icons } from './NavIcons';
import GitGraphPanel from './GitGraphPanel';
import CommandPalette from './CommandPalette';
import GlobalSearch from './GlobalSearch';
import { NotificationsDrawer } from './NotificationsDrawer';
import RouteLoadingSkeleton from './RouteLoadingSkeleton';
import { StatusBadge } from './StatusBadge';
import { useNotificationStore } from '../stores/notificationStore';
import { useNotificationCenter } from '../hooks/useNotificationCenter';
import { getHelpSectionForRoute } from '../pages/helpDocs';
import {
  claimProjectOwnership,
  getHealth,
  getProjectOwnership,
  getWebClientId,
  heartbeatProjectOwnership,
  requestProjectTakeover,
  respondProjectTakeover,
  setWebOwnershipMode,
} from '../api/client';
import type { ApiOwnershipRecord } from '../api/models';
import { useCoordinatorStore } from '../store';

const navGroups = [
  {
    title: 'Setup & Config',
    items: [
      { path: '/welcome', label: 'Welcome', icon: Icons.Activity },
      { path: '/init', label: 'Init', icon: Icons.Terminal },
      { path: '/dashboard', label: 'Dashboard', icon: Icons.Home },
      { path: '/config/tools', label: 'Tools', icon: Icons.Wrench },
      { path: '/config/standards', label: 'Standards', icon: Icons.CheckSquare },
      { path: '/config/skills', label: 'Skills', icon: Icons.Brain },
      { path: '/config/settings', label: 'Settings', icon: Icons.Settings },
      { path: '/prd', label: 'PRD', icon: Icons.FileText },
      { path: '/plan', label: 'Plan', icon: Icons.List },
      { path: '/apply', label: 'Apply', icon: Icons.Play },
    ]
  },
  {
    title: 'Ops',
    items: [
      { path: '/ops/console', label: 'Console', icon: Icons.Terminal },
      { path: '/ops/registry', label: 'Registry', icon: Icons.Database },
      { path: '/ops/worktrees', label: 'Worktrees', icon: Icons.Folder },
      { path: '/ops/live', label: 'Live', icon: Icons.Activity },
      { path: '/ops/locks', label: 'Locks', icon: Icons.Lock },
      { path: '/ops/diagnostics', label: 'Diagnostics', icon: Icons.Stethoscope },
      { path: '/ops/logs', label: 'Logs', icon: Icons.AlignLeft },
      { path: '/ops/backups', label: 'Backups', icon: Icons.Archive },
      { path: '/ops/git', label: 'Git Graph', icon: Icons.Activity },
      { path: '/ops/trust', label: 'Trust & Safety', icon: Icons.Shield },
      { path: '/ops/skill-runner', label: 'Skill Runner', icon: Icons.Activity },
    ]
  },
  {
    title: 'Support',
    items: [
      { path: '/help', label: 'Help', icon: Icons.Search },
      { path: '/about', label: 'About', icon: Icons.Activity },
    ]
  }
];

const LAYOUT_REFRESH_MS = 10_000;
const OWNERSHIP_REFRESH_MS = 2_000;
const OWNERSHIP_HEARTBEAT_MS = 15_000;

const Layout: React.FC = () => {
  const [isCollapsed, setIsCollapsed] = useState(false);
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false);
  const [projectRoot, setProjectRoot] = useState<string | null>(null);
  const [ownership, setOwnership] = useState<ApiOwnershipRecord | null>(null);
  const [ownershipMessage, setOwnershipMessage] = useState<string | null>(null);
  const [webClientId] = useState(() => getWebClientId());
  const mainRef = useRef<HTMLElement>(null);
  const location = useLocation();
  const contextualHelpSection = getHelpSectionForRoute(location.pathname);
  const contextualHelpHref = `/help?section=${encodeURIComponent(contextualHelpSection)}`;
  const showGitPanel = !location.pathname.startsWith('/ops/git');
  const unreadCount = useNotificationStore((state) => state.unreadCount);
  const setIsOpen = useNotificationStore((state) => state.setIsOpen);
  const status = useCoordinatorStore((state) => state.status);
  const loadStatus = useCoordinatorStore((state) => state.loadStatus);

  // Initialize global notification center
  useNotificationCenter();

  // Fetch project root once from health endpoint
  useEffect(() => {
    getHealth().then((h) => {
      if (h.project_root) setProjectRoot(h.project_root);
    }).catch(() => null);
  }, []);

  // Poll coordinator status for the status strip
  useEffect(() => {
    const abortController = new AbortController();
    void loadStatus(abortController.signal).catch(() => undefined);
    const id = setInterval(() => {
      void loadStatus().catch(() => undefined);
    }, LAYOUT_REFRESH_MS);
    return () => {
      abortController.abort();
      clearInterval(id);
    };
  }, [loadStatus]);

  useEffect(() => {
    let disposed = false;
    const clientId = webClientId;
    const refreshOwnership = async () => {
      try {
        await claimProjectOwnership({ clientId });
        const record = await getProjectOwnership();
        if (!disposed) {
          setOwnership(record);
          setWebOwnershipMode(record?.owner?.client_id === clientId ? 'owner' : 'viewer');
          setOwnershipMessage(null);
        }
      } catch (error) {
        if (!disposed) {
          setWebOwnershipMode('unknown');
          setOwnershipMessage(error instanceof Error ? error.message : 'Ownership refresh failed.');
        }
      }
    };

    void refreshOwnership();
    const refreshId = window.setInterval(() => void refreshOwnership(), OWNERSHIP_REFRESH_MS);
    const heartbeatId = window.setInterval(() => {
      void heartbeatProjectOwnership({ clientId }).catch(() => undefined);
    }, OWNERSHIP_HEARTBEAT_MS);

    return () => {
      disposed = true;
      window.clearInterval(refreshId);
      window.clearInterval(heartbeatId);
    };
  }, [webClientId]);

  const isOwner = ownership?.owner?.client_id === webClientId;
  const pendingTakeover = ownership?.takeover_request ?? null;
  const requestTakeover = () => {
    requestProjectTakeover({ clientId: webClientId })
      .then(() => setOwnershipMessage('Takeover request sent.'))
      .catch((error) =>
        setOwnershipMessage(error instanceof Error ? error.message : 'Takeover request failed.'),
      );
  };
  const respondTakeover = (accept: boolean) => {
    if (!pendingTakeover) return;
    respondProjectTakeover(pendingTakeover.request_id, accept, { clientId: webClientId })
      .then(() => setOwnershipMessage(accept ? 'Takeover accepted.' : 'Takeover rejected.'))
      .catch((error) =>
        setOwnershipMessage(error instanceof Error ? error.message : 'Takeover response failed.'),
      );
  };

  React.useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        setIsCommandPaletteOpen(true);
      }
    };

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  useEffect(() => {
    mainRef.current?.focus();
  }, [location.pathname]);

  return (
    <div className="relative flex h-screen w-full overflow-hidden bg-[var(--bg-primary)] text-[var(--text-primary)]">
      <a
        href="#main-content"
        className="fixed left-4 top-4 z-50 -translate-y-20 rounded-md bg-[var(--accent)] px-4 py-2 text-sm font-semibold text-white shadow-lg transition-transform focus:translate-y-0 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/80"
      >
        Skip to content
      </a>

      {/* Sidebar */}
      <aside
        className={`flex flex-col border-r border-[var(--border)] bg-[var(--bg-secondary)] transition-all duration-300 ${
          isCollapsed ? 'w-[68px]' : 'w-[240px]'
        }`}
      >
        <nav aria-label="Primary navigation" className="flex h-full min-h-0 flex-col">
          <div className="flex h-12 items-center justify-between border-b border-[var(--border)] px-4">
            {!isCollapsed && <span className="text-sm font-bold uppercase tracking-wider text-[var(--text-secondary)]">MACC</span>}
            <button
              aria-label={isCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
              className={`rounded p-1 text-[var(--text-muted)] transition-colors hover:bg-[var(--bg-card)] hover:text-[var(--text-primary)] ${
                isCollapsed ? 'mx-auto' : ''
              }`}
              onClick={() => setIsCollapsed(!isCollapsed)}
              type="button"
            >
              {isCollapsed ? <Icons.ChevronRight /> : <Icons.ChevronLeft />}
            </button>
          </div>

          <div className="flex-1 overflow-y-auto py-4 scrollbar-thin">
          {navGroups.map((group, idx) => (
              <div key={idx} className="mb-6">
                {!isCollapsed && (
                  <div className="mb-2 px-4 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
                    {group.title}
                  </div>
                )}
                {isCollapsed && <div className="h-4" />}

                <ul className="space-y-1 px-2">
                  {group.items.map((item) => {
                    const isActive = location.pathname.startsWith(item.path);
                    const Icon = item.icon;

                    return (
                      <li key={item.path}>
                        <Link
                          aria-current={isActive ? 'page' : undefined}
                          className={`flex items-center gap-3 rounded-md px-3 py-2 transition-colors ${
                            isActive
                              ? 'bg-[var(--bg-card)] text-[var(--accent)]'
                              : 'text-[var(--text-secondary)] hover:bg-[var(--bg-card)] hover:text-[var(--text-primary)]'
                          } ${isCollapsed ? 'justify-center' : ''}`}
                          to={item.path}
                          title={isCollapsed ? item.label : undefined}
                        >
                          <div className={`flex-shrink-0 ${isActive ? 'text-[var(--accent)]' : ''}`}>
                            <Icon />
                          </div>
                          <span className={isCollapsed ? 'sr-only' : 'whitespace-nowrap text-sm font-medium'}>
                            {item.label}
                          </span>
                        </Link>
                      </li>
                    );
                  })}
                </ul>
              </div>
            ))}
          </div>
        </nav>
      </aside>

      {/* Main Content Area */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Top Bar */}
        <header className="h-12 bg-[var(--bg-primary)] border-b border-[var(--border)] flex items-center justify-between px-4 shrink-0">
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2">
              <StatusBadge className="shrink-0 px-2 py-0.5 text-[11px]" status="Connected" tone="active" />
              {projectRoot && (
                <span className="max-w-[400px] truncate font-mono text-sm text-[var(--text-secondary)]">
                  {projectRoot}
                </span>
              )}
            </div>
          </div>
          
          <div className="flex flex-1 items-center justify-center px-4">
            <GlobalSearch />
          </div>

          <div className="flex items-center gap-3">
            <div className="hidden items-center gap-2 rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-2 py-1 text-xs text-[var(--text-secondary)] xl:flex">
              <span className="uppercase tracking-wide">Control:</span>
              <StatusBadge
                className="px-2 py-0.5 text-[10px]"
                status={isOwner ? 'Owner' : 'Viewer'}
                tone={isOwner ? 'active' : 'todo'}
              />
              <span
                className="max-w-[180px] truncate font-mono"
                title={ownership?.owner?.client_id ?? 'unclaimed'}
              >
                {ownership?.owner?.client_id ?? 'unclaimed'}
              </span>
              {!isOwner && (
                <button
                  className="rounded border border-[var(--border)] px-2 py-0.5 text-[11px] text-[var(--text-primary)] hover:border-[var(--accent)]"
                  onClick={requestTakeover}
                  type="button"
                >
                  Request
                </button>
              )}
              {isOwner && pendingTakeover && (
                <span className="flex items-center gap-1">
                  <button
                    className="rounded border border-emerald-500/50 px-2 py-0.5 text-[11px] text-emerald-300"
                    onClick={() => respondTakeover(true)}
                    type="button"
                  >
                    Accept
                  </button>
                  <button
                    className="rounded border border-rose-500/50 px-2 py-0.5 text-[11px] text-rose-300"
                    onClick={() => respondTakeover(false)}
                    type="button"
                  >
                    Reject
                  </button>
                </span>
              )}
            </div>
            <Link
              aria-label="Open contextual help"
              className="flex h-9 w-9 items-center justify-center rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] text-base font-semibold text-[var(--text-muted)] transition-colors hover:border-[var(--text-muted)] hover:text-[var(--text-primary)]"
              title="Contextual help"
              to={contextualHelpHref}
            >
              ?
            </Link>
            <button 
              onClick={() => setIsOpen(true)}
              className="relative flex h-9 w-9 items-center justify-center rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] text-[var(--text-muted)] transition-colors hover:border-[var(--text-muted)] hover:text-[var(--text-primary)]"
              aria-label="Open notifications"
              title="Notifications"
              type="button"
            >
              <Icons.Bell />
              {unreadCount > 0 && (
                <span className="absolute -top-1 -right-1 flex h-4 w-4 items-center justify-center rounded-full bg-rose-500 text-[9px] font-bold text-white shadow-sm ring-2 ring-[var(--bg-primary)]">
                  {unreadCount > 99 ? '9+' : unreadCount}
                </span>
              )}
            </button>
            <button
              className="flex items-center gap-2 rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-1.5 text-sm text-[var(--text-muted)] transition-colors hover:border-[var(--text-muted)] hover:text-[var(--text-primary)]"
              onClick={() => setIsCommandPaletteOpen(true)}
              type="button"
            >
              <Icons.Search />
              <span>Search...</span>
              <kbd className="ml-2 rounded border border-[var(--border)] bg-[var(--bg-card)] px-1.5 py-0.5 font-mono text-xs">
                Ctrl+K
              </kbd>
            </button>
          </div>
        </header>

        {/* Page Content + Git Graph Side Panel */}
        <div className="flex min-h-0 flex-1">
          <main
            ref={mainRef}
            id="main-content"
            tabIndex={-1}
            className="flex-1 overflow-auto bg-[var(--bg-primary)] p-6 focus:outline-none"
          >
            <React.Suspense fallback={<RouteLoadingSkeleton />}>
              <Outlet />
            </React.Suspense>
          </main>
          {showGitPanel && <GitGraphPanel />}
        </div>

        {/* Status Strip */}
        <footer className="h-8 bg-[var(--bg-secondary)] border-t border-[var(--border)] flex items-center px-4 text-xs font-mono text-[var(--text-muted)] shrink-0 justify-between">
          <div className="flex items-center gap-4">
            <div className="flex items-center gap-2">
              <span className="uppercase text-[var(--text-secondary)]">Coordinator:</span>
              <StatusBadge
                className="px-2 py-0.5 text-[11px]"
                status={status ? (status.paused ? 'Paused' : status.active > 0 ? 'Running' : 'Idle') : 'Idle'}
                tone={status ? (status.paused ? 'paused' : status.active > 0 ? 'active' : 'todo') : 'todo'}
              />
            </div>
            {ownershipMessage && <span className="max-w-[420px] truncate text-amber-300">{ownershipMessage}</span>}
          </div>
          <div className="flex items-center gap-6">
            <div className="flex items-center gap-1.5">
              <span>Active Workers:</span>
              <span className="text-[var(--text-primary)]">{status?.active ?? 0}</span>
            </div>
            <div className="flex items-center gap-1.5">
              <span>Throttled Tools:</span>
              <span className="text-[var(--text-primary)]">{status?.throttled_tools?.length ?? 0}</span>
            </div>
          </div>
        </footer>
      </div>
      <NotificationsDrawer />
      <CommandPalette open={isCommandPaletteOpen} onOpenChange={setIsCommandPaletteOpen} />
    </div>
  );
};

export default Layout;
