import React, { useEffect, useRef, useState } from 'react';
import { Link, Outlet, useLocation } from 'react-router-dom';
import { Icons } from './NavIcons';
import GitGraphPanel from './GitGraphPanel';
import CommandPalette from './CommandPalette';
import { NotificationsDrawer } from './NotificationsDrawer';
import RouteLoadingSkeleton from './RouteLoadingSkeleton';
import { StatusBadge } from './StatusBadge';
import { useNotificationStore } from '../stores/notificationStore';
import { useNotificationCenter } from '../hooks/useNotificationCenter';
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

/* ── Navigation structure ────────────────────────────────────────── */
interface NavItem { path: string; label: string; icon: React.FC; }
interface NavSection { label?: string; items: NavItem[]; }

const NAV_SECTIONS: NavSection[] = [
  {
    items: [
      { path: '/dashboard', label: 'Dashboard', icon: Icons.Dashboard },
    ],
  },
  {
    label: 'Setup',
    items: [
      { path: '/welcome',            label: 'Welcome',   icon: Icons.Home        },
      { path: '/init',               label: 'Init',      icon: Icons.Terminal    },
      { path: '/config/tools',       label: 'Tools',     icon: Icons.Wrench      },
      { path: '/config/standards',   label: 'Standards', icon: Icons.CheckSquare },
      { path: '/config/skills',      label: 'Skills',    icon: Icons.Brain       },
      { path: '/config/settings',    label: 'Settings',  icon: Icons.Settings    },
    ],
  },
  {
    label: 'Pipeline',
    items: [
      { path: '/prd',   label: 'PRD',   icon: Icons.FileText },
      { path: '/plan',  label: 'Plan',  icon: Icons.List     },
      { path: '/apply', label: 'Apply', icon: Icons.Rocket   },
    ],
  },
  {
    label: 'Runtime',
    items: [
      { path: '/ops/console',   label: 'Console',   icon: Icons.Terminal  },
      { path: '/ops/worktrees', label: 'Worktrees', icon: Icons.Folder    },
      { path: '/ops/registry',  label: 'Registry',  icon: Icons.Database  },
      { path: '/ops/locks',     label: 'Locks',     icon: Icons.Lock      },
    ],
  },
  {
    label: 'Observe',
    items: [
      { path: '/ops/logs',            label: 'Logs',         icon: Icons.AlignLeft    },
      { path: '/ops/diagnostics',     label: 'Diagnostics',  icon: Icons.Stethoscope  },
      { path: '/ops/git',             label: 'Git Graph',    icon: Icons.Git          },
      { path: '/ops/backups',         label: 'Backups',      icon: Icons.Archive      },
      { path: '/ops/trust',           label: 'Trust',        icon: Icons.Shield       },
      { path: '/ops/skill-runner',    label: 'Skill Runner', icon: Icons.Zap          },
      { path: '/ops/skills-catalog',  label: 'Catalog',      icon: Icons.Brain        },
    ],
  },
  {
    label: 'Support',
    items: [
      { path: '/help',  label: 'Help',  icon: Icons.BookOpen },
      { path: '/about', label: 'About', icon: Icons.Info     },
    ],
  },
];

/* ── Inline style helpers ────────────────────────────────────────── */
const topbarH    = 'var(--topbar-height)';
const statusbarH = 'var(--statusbar-height)';

/* ── Timing ──────────────────────────────────────────────────────── */
const LAYOUT_REFRESH_MS      = 10_000;
const OWNERSHIP_REFRESH_MS   =  2_000;
const OWNERSHIP_HEARTBEAT_MS = 15_000;

/* ── Component ───────────────────────────────────────────────────── */
const Layout: React.FC = () => {
  const [collapsed, setCollapsed]               = useState(false);
  const [cmdOpen, setCmdOpen]                   = useState(false);
  const [projectRoot, setProjectRoot]           = useState<string | null>(null);
  const [ownership, setOwnership]               = useState<ApiOwnershipRecord | null>(null);
  const [ownershipMessage, setOwnershipMessage] = useState<string | null>(null);
  const [webClientId]                           = useState(() => getWebClientId());
  const mainRef                                 = useRef<HTMLElement>(null);
  const location                                = useLocation();

  const unreadCount = useNotificationStore((s) => s.unreadCount);
  const setIsOpen   = useNotificationStore((s) => s.setIsOpen);
  const status      = useCoordinatorStore((s) => s.status);
  const loadStatus  = useCoordinatorStore((s) => s.loadStatus);

  const isOwner        = ownership?.owner?.client_id === webClientId;
  const pendingTakeover = ownership?.takeover_request ?? null;
  const showGitPanel   = !location.pathname.startsWith('/ops/git');

  useNotificationCenter();

  useEffect(() => {
    getHealth().then((h) => { if (h.project_root) setProjectRoot(h.project_root); }).catch(() => null);
  }, []);

  useEffect(() => {
    const ctrl = new AbortController();
    void loadStatus(ctrl.signal).catch(() => undefined);
    const id = setInterval(() => void loadStatus().catch(() => undefined), LAYOUT_REFRESH_MS);
    return () => { ctrl.abort(); clearInterval(id); };
  }, [loadStatus]);

  useEffect(() => {
    let disposed = false;
    const clientId = webClientId;
    const refresh = async () => {
      try {
        await claimProjectOwnership({ clientId });
        const record = await getProjectOwnership();
        if (!disposed) {
          setOwnership(record);
          setWebOwnershipMode(record?.owner?.client_id === clientId ? 'owner' : 'viewer');
        }
      } catch {
        if (!disposed) setWebOwnershipMode('unknown');
      }
    };
    const hb = async () => {
      try { await heartbeatProjectOwnership({ clientId }); } catch { /* silent */ }
    };
    void refresh();
    const rId = setInterval(refresh, OWNERSHIP_REFRESH_MS);
    const hId = setInterval(hb,      OWNERSHIP_HEARTBEAT_MS);
    return () => { disposed = true; clearInterval(rId); clearInterval(hId); };
  }, [webClientId]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'k') {
        e.preventDefault();
        setCmdOpen((v) => !v);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []);

  const requestTakeover = async () => {
    try {
      await requestProjectTakeover({ clientId: webClientId });
      setOwnershipMessage('Takeover requested — waiting for the owner to respond.');
      setTimeout(() => setOwnershipMessage(null), 6000);
    } catch { /* silent */ }
  };

  const respondTakeover = (accept: boolean) => {
    if (!pendingTakeover) return;
    respondProjectTakeover(pendingTakeover.request_id, accept, { clientId: webClientId })
      .then(() => {
        setOwnershipMessage(accept ? 'Ownership transferred.' : 'Takeover rejected.');
        setTimeout(() => setOwnershipMessage(null), 4000);
      })
      .catch(() => { /* silent */ });
  };

  const coordState =
    status?.paused         ? { label: 'Paused',  tone: 'paused' as const } :
    (status?.active ?? 0) > 0 ? { label: 'Running', tone: 'active' as const } :
                              { label: 'Idle',    tone: 'todo'   as const };

  const activeWorkers  = status?.active ?? 0;
  const throttledCount = status?.throttled_tools?.length ?? 0;

  return (
    <div style={{ display: 'flex', height: '100vh', overflow: 'hidden', background: 'var(--bg-primary)', color: 'var(--text-primary)' }}>

      {/* ── Sidebar ────────────────────────────────────────────── */}
      <aside
        className="sidebar-transition"
        style={{
          display: 'flex',
          flexDirection: 'column',
          flexShrink: 0,
          overflow: 'hidden',
          width: collapsed ? 'var(--sidebar-width-collapsed)' : 'var(--sidebar-width)',
          background: 'var(--bg-secondary)',
          borderRight: '1px solid var(--border-subtle)',
        }}
      >
        {/* Logo / header row */}
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            flexShrink: 0,
            height: topbarH,
            padding: collapsed ? '0' : '0 10px 0 14px',
            justifyContent: collapsed ? 'center' : 'space-between',
            borderBottom: '1px solid var(--border-subtle)',
          }}
        >
          {!collapsed && (
            <span
              style={{
                fontSize: '14px',
                fontWeight: 600,
                letterSpacing: '-0.03em',
                color: 'var(--text-primary)',
                userSelect: 'none',
              }}
            >
              MACC
            </span>
          )}
          <button
            type="button"
            aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            onClick={() => setCollapsed((v) => !v)}
            style={{
              display: 'flex', alignItems: 'center', justifyContent: 'center',
              width: 28, height: 28,
              background: 'none', border: 'none', cursor: 'pointer',
              color: 'var(--text-muted)',
              borderRadius: 'var(--radius-sm)',
              transition: 'color 100ms ease, background 100ms ease',
            }}
            onMouseEnter={(e) => { const el = e.currentTarget; el.style.color = 'var(--text-primary)'; el.style.background = 'var(--bg-elevated)'; }}
            onMouseLeave={(e) => { const el = e.currentTarget; el.style.color = 'var(--text-muted)'; el.style.background = 'none'; }}
          >
            {collapsed ? <Icons.ChevronRight /> : <Icons.ChevronLeft />}
          </button>
        </div>

        {/* Navigation */}
        <nav
          aria-label="Main navigation"
          style={{ flex: 1, overflowY: 'auto', padding: '6px 0 8px' }}
        >
          {NAV_SECTIONS.map((section, si) => (
            <div key={si}>
              {/* Divider between sections */}
              {si > 0 && (
                <div
                  style={{
                    height: 1,
                    margin: collapsed ? '8px 10px' : '6px 0',
                    background: 'var(--border-subtle)',
                  }}
                />
              )}

              {/* Section label */}
              {section.label && !collapsed && (
                <div
                  style={{
                    fontSize: '10px',
                    fontWeight: 500,
                    color: 'var(--text-muted)',
                    padding: '4px 14px 3px',
                    userSelect: 'none',
                    letterSpacing: '0.01em',
                  }}
                >
                  {section.label}
                </div>
              )}

              {/* Nav items */}
              <ul style={{ listStyle: 'none', margin: 0, padding: '0 5px' }}>
                {section.items.map((item) => {
                  const exact = item.path === '/dashboard';
                  const active = exact
                    ? (location.pathname === '/dashboard' || location.pathname === '/')
                    : location.pathname.startsWith(item.path);
                  const Icon = item.icon;

                  return (
                    <li key={item.path}>
                      <Link
                        to={item.path}
                        aria-current={active ? 'page' : undefined}
                        title={collapsed ? item.label : undefined}
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          gap: 8,
                          padding: collapsed ? '6px 0' : '5px 9px',
                          justifyContent: collapsed ? 'center' : 'flex-start',
                          textDecoration: 'none',
                          fontSize: 'var(--text-sm)',
                          fontWeight: active ? 500 : 400,
                          color: active ? 'var(--accent)' : 'var(--text-secondary)',
                          background: active ? 'var(--accent-bg)' : 'transparent',
                          borderRadius: 'var(--radius-sm)',
                          marginBottom: 1,
                          transition: 'color 100ms ease, background 100ms ease',
                        }}
                        onMouseEnter={(e) => {
                          if (!active) {
                            const el = e.currentTarget as HTMLElement;
                            el.style.background = 'var(--bg-elevated)';
                            el.style.color = 'var(--text-primary)';
                          }
                        }}
                        onMouseLeave={(e) => {
                          if (!active) {
                            const el = e.currentTarget as HTMLElement;
                            el.style.background = 'transparent';
                            el.style.color = 'var(--text-secondary)';
                          }
                        }}
                      >
                        <span style={{ color: active ? 'var(--accent)' : 'inherit', flexShrink: 0, display: 'flex' }}>
                          <Icon />
                        </span>
                        {!collapsed && (
                          <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                            {item.label}
                          </span>
                        )}
                      </Link>
                    </li>
                  );
                })}
              </ul>
            </div>
          ))}
        </nav>

        {/* Ownership footer in sidebar */}
        {!collapsed && (
          <div
            style={{
              borderTop: '1px solid var(--border-subtle)',
              padding: '7px 12px',
              fontSize: '11px',
              color: 'var(--text-muted)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 8,
              minHeight: 36,
            }}
          >
            <span style={{ color: isOwner ? 'var(--success)' : 'var(--text-muted)' }}>
              {isOwner ? 'Owner' : 'Viewer'}
            </span>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              {!isOwner && (
                <button
                  type="button"
                  onClick={requestTakeover}
                  style={{
                    fontSize: '11px', color: 'var(--accent)',
                    background: 'none', border: 'none', cursor: 'pointer', padding: '1px 4px',
                  }}
                >
                  Request control
                </button>
              )}
              {isOwner && pendingTakeover && (
                <>
                  <button
                    type="button"
                    onClick={() => void respondTakeover(true)}
                    style={{ fontSize: '11px', color: 'var(--success)', background: 'none', border: 'none', cursor: 'pointer' }}
                  >
                    Accept
                  </button>
                  <button
                    type="button"
                    onClick={() => void respondTakeover(false)}
                    style={{ fontSize: '11px', color: 'var(--error)', background: 'none', border: 'none', cursor: 'pointer' }}
                  >
                    Reject
                  </button>
                </>
              )}
            </div>
          </div>
        )}
      </aside>

      {/* ── Main column ─────────────────────────────────────────── */}
      <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minWidth: 0 }}>

        {/* Top bar */}
        <header
          style={{
            display: 'flex',
            alignItems: 'center',
            height: topbarH,
            flexShrink: 0,
            background: 'var(--bg-secondary)',
            borderBottom: '1px solid var(--border-subtle)',
            padding: '0 14px',
            gap: 12,
          }}
        >
          {/* Project root path */}
          <div style={{ flex: 1, minWidth: 0, display: 'flex', alignItems: 'center', gap: 8 }}>
            {projectRoot ? (
              <span
                title={projectRoot}
                style={{
                  fontFamily: 'var(--font-mono)',
                  fontSize: '11px',
                  color: 'var(--text-muted)',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                  maxWidth: 400,
                }}
              >
                {projectRoot}
              </span>
            ) : (
              <span style={{ fontSize: '11px', color: 'var(--text-muted)' }}>No project loaded</span>
            )}
          </div>

          {/* Coordinator status — center */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0 }}>
            <StatusBadge status={coordState.label} tone={coordState.tone} className="px-2 py-0.5 text-[11px]" />
            {activeWorkers > 0 && (
              <span style={{ fontSize: '11px', color: 'var(--text-muted)', fontFamily: 'var(--font-mono)' }}>
                {activeWorkers}w
              </span>
            )}
            {throttledCount > 0 && (
              <span style={{ fontSize: '11px', color: 'var(--warning)', fontFamily: 'var(--font-mono)' }}>
                {throttledCount} throttled
              </span>
            )}
          </div>

          {/* Right: notifications + search */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexShrink: 0 }}>
            {/* Notifications */}
            <button
              type="button"
              aria-label="Open notifications"
              onClick={() => setIsOpen(true)}
              style={{
                position: 'relative',
                display: 'flex', alignItems: 'center', justifyContent: 'center',
                width: 30, height: 30,
                background: 'none', border: 'none', cursor: 'pointer',
                color: 'var(--text-muted)',
                borderRadius: 'var(--radius-sm)',
                transition: 'color 100ms ease, background 100ms ease',
              }}
              onMouseEnter={(e) => { const el = e.currentTarget; el.style.color = 'var(--text-primary)'; el.style.background = 'var(--bg-elevated)'; }}
              onMouseLeave={(e) => { const el = e.currentTarget; el.style.color = 'var(--text-muted)'; el.style.background = 'none'; }}
            >
              <Icons.Bell />
              {unreadCount > 0 && (
                <span
                  style={{
                    position: 'absolute', top: 3, right: 3,
                    width: 14, height: 14,
                    display: 'flex', alignItems: 'center', justifyContent: 'center',
                    borderRadius: '50%',
                    background: 'var(--error)',
                    color: '#fff',
                    fontSize: 9,
                    fontWeight: 700,
                    boxShadow: '0 0 0 2px var(--bg-secondary)',
                  }}
                >
                  {unreadCount > 9 ? '9+' : unreadCount}
                </span>
              )}
            </button>

            {/* Command palette */}
            <button
              type="button"
              onClick={() => setCmdOpen(true)}
              style={{
                display: 'flex', alignItems: 'center', gap: 6,
                padding: '4px 10px',
                background: 'var(--bg-card)',
                border: '1px solid var(--border)',
                borderRadius: 'var(--radius-md)',
                fontSize: '12px',
                color: 'var(--text-muted)',
                cursor: 'pointer',
                fontFamily: 'var(--font-ui)',
                whiteSpace: 'nowrap',
                transition: 'color 100ms ease, border-color 100ms ease',
              }}
              onMouseEnter={(e) => { const el = e.currentTarget; el.style.color = 'var(--text-primary)'; el.style.borderColor = 'var(--text-muted)'; }}
              onMouseLeave={(e) => { const el = e.currentTarget; el.style.color = 'var(--text-muted)'; el.style.borderColor = 'var(--border)'; }}
            >
              <Icons.Search />
              <span>Search</span>
              <kbd
                style={{
                  fontFamily: 'var(--font-mono)',
                  fontSize: 10,
                  padding: '1px 5px',
                  background: 'var(--bg-elevated)',
                  border: '1px solid var(--border)',
                  borderRadius: 3,
                  color: 'var(--text-muted)',
                  marginLeft: 2,
                }}
              >
                ⌘K
              </kbd>
            </button>
          </div>
        </header>

        {/* Ownership message banner */}
        {ownershipMessage && (
          <div
            role="status"
            style={{
              background: 'oklch(0.75 0.17 80 / 0.15)',
              borderBottom: '1px solid oklch(0.75 0.17 80 / 0.3)',
              color: 'var(--warning)',
              fontSize: '12px',
              padding: '5px 16px',
              textAlign: 'center',
              fontWeight: 500,
              flexShrink: 0,
            }}
          >
            {ownershipMessage}
          </div>
        )}

        {/* Content + optional git panel */}
        <div style={{ display: 'flex', flex: 1, minHeight: 0 }}>
          <main
            ref={mainRef}
            id="main-content"
            tabIndex={-1}
            style={{
              flex: 1,
              overflowY: 'auto',
              background: 'var(--bg-primary)',
              padding: '20px 24px',
              outline: 'none',
            }}
          >
            <React.Suspense fallback={<RouteLoadingSkeleton />}>
              <Outlet />
            </React.Suspense>
          </main>
          {showGitPanel && <GitGraphPanel />}
        </div>

        {/* Status bar */}
        <footer
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            height: statusbarH,
            flexShrink: 0,
            background: 'var(--bg-secondary)',
            borderTop: '1px solid var(--border-subtle)',
            padding: '0 14px',
            fontFamily: 'var(--font-mono)',
            fontSize: '10px',
            color: 'var(--text-muted)',
            gap: 12,
            userSelect: 'none',
          }}
        >
          {/* Left: coordinator pulse */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span
              style={{
                color:
                  coordState.tone === 'active' ? 'var(--success)' :
                  coordState.tone === 'paused' ? 'var(--warning)' :
                  'var(--text-muted)',
              }}
            >
              ● {coordState.label}
            </span>
            <span>{activeWorkers} active · {throttledCount} throttled</span>
          </div>

          {/* Right: path + ownership */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 12, minWidth: 0 }}>
            {projectRoot && (
              <span
                title={projectRoot}
                style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 320 }}
              >
                {projectRoot}
              </span>
            )}
            <span style={{ color: isOwner ? 'var(--success)' : 'var(--text-muted)', flexShrink: 0 }}>
              {isOwner ? 'Owner' : 'Viewer'}
            </span>
          </div>
        </footer>
      </div>

      {/* Global overlays */}
      <NotificationsDrawer />
      <CommandPalette open={cmdOpen} onOpenChange={setCmdOpen} />
    </div>
  );
};

export default Layout;
