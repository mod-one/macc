import React, { useState } from 'react';
import { Link, Outlet, useLocation } from 'react-router-dom';
import { Icons } from './Icons';

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
      { path: '/ops/live', label: 'Live', icon: Icons.Activity },
      { path: '/ops/locks', label: 'Locks', icon: Icons.Lock },
      { path: '/ops/diagnostics', label: 'Diagnostics', icon: Icons.Stethoscope },
      { path: '/ops/logs', label: 'Logs', icon: Icons.AlignLeft },
      { path: '/ops/backups', label: 'Backups', icon: Icons.Archive },
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

const Layout: React.FC = () => {
  const [isCollapsed, setIsCollapsed] = useState(false);
  const location = useLocation();

  return (
    <div className="flex h-screen w-full bg-[var(--bg-primary)] text-[var(--text-primary)] overflow-hidden">
      {/* Sidebar */}
      <aside 
        className={`flex flex-col bg-[var(--bg-secondary)] border-r border-[var(--border)] transition-all duration-300 ${
          isCollapsed ? 'w-[68px]' : 'w-[240px]'
        }`}
      >
        <div className="h-12 flex items-center justify-between px-4 border-b border-[var(--border)]">
          {!isCollapsed && <span className="font-bold text-sm tracking-wider uppercase text-[var(--text-secondary)]">MACC</span>}
          <button 
            onClick={() => setIsCollapsed(!isCollapsed)}
            className={`p-1 rounded hover:bg-[var(--bg-card)] text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors ${isCollapsed ? 'mx-auto' : ''}`}
            title={isCollapsed ? "Expand Sidebar" : "Collapse Sidebar"}
          >
            {isCollapsed ? <Icons.ChevronRight /> : <Icons.ChevronLeft />}
          </button>
        </div>

        <div className="flex-1 overflow-y-auto py-4 scrollbar-thin">
          {navGroups.map((group, idx) => (
            <div key={idx} className="mb-6">
              {!isCollapsed && (
                <div className="px-4 mb-2 text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wider">
                  {group.title}
                </div>
              )}
              {isCollapsed && <div className="h-4" /> /* Spacer for collapsed state to maintain grouping visual rhythm */}
              
              <ul className="space-y-1 px-2">
                {group.items.map(item => {
                  const isActive = location.pathname.startsWith(item.path);
                  const Icon = item.icon;
                  
                  return (
                    <li key={item.path}>
                      <Link
                        to={item.path}
                        title={isCollapsed ? item.label : undefined}
                        className={`flex items-center gap-3 px-3 py-2 rounded-md transition-colors ${
                          isActive 
                            ? 'bg-[var(--bg-card)] text-[var(--accent)]' 
                            : 'text-[var(--text-secondary)] hover:bg-[var(--bg-card)] hover:text-[var(--text-primary)]'
                        } ${isCollapsed ? 'justify-center' : ''}`}
                      >
                        <div className={`flex-shrink-0 ${isActive ? 'text-[var(--accent)]' : ''}`}>
                          <Icon />
                        </div>
                        {!isCollapsed && (
                          <span className="text-sm font-medium whitespace-nowrap">
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
        </div>
      </aside>

      {/* Main Content Area */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Top Bar */}
        <header className="h-12 bg-[var(--bg-primary)] border-b border-[var(--border)] flex items-center justify-between px-4 shrink-0">
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2">
              <div className="w-2.5 h-2.5 rounded-full bg-[var(--success)] shadow-[0_0_8px_var(--success)]" title="Connected"></div>
              <span className="text-sm font-mono text-[var(--text-secondary)] truncate max-w-[400px]">
                /home/brand/macc/.macc/worktree/worker-03
              </span>
            </div>
          </div>
          
          <div className="flex items-center">
            <button className="flex items-center gap-2 px-3 py-1.5 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-md text-sm text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:border-[var(--text-muted)] transition-colors">
              <Icons.Search />
              <span>Search...</span>
              <kbd className="ml-2 font-mono text-xs bg-[var(--bg-card)] px-1.5 py-0.5 rounded border border-[var(--border)]">Ctrl+K</kbd>
            </button>
          </div>
        </header>

        {/* Page Content */}
        <main className="flex-1 overflow-auto bg-[var(--bg-primary)] p-6">
          <React.Suspense fallback={<div className="flex items-center justify-center h-full text-[var(--text-muted)] animate-pulse">Loading...</div>}>
            <Outlet />
          </React.Suspense>
        </main>

        {/* Status Strip */}
        <footer className="h-8 bg-[var(--bg-secondary)] border-t border-[var(--border)] flex items-center px-4 text-xs font-mono text-[var(--text-muted)] shrink-0 justify-between">
          <div className="flex items-center gap-4">
            <div className="flex items-center gap-1.5">
              <span className="uppercase text-[var(--text-secondary)]">Coordinator:</span>
              <span className="text-[var(--status-active)]">IDLE</span>
            </div>
          </div>
          <div className="flex items-center gap-6">
            <div className="flex items-center gap-1.5">
              <span>Active Workers:</span>
              <span className="text-[var(--text-primary)]">0</span>
            </div>
            <div className="flex items-center gap-1.5">
              <span>Throttled Tools:</span>
              <span className="text-[var(--text-primary)]">0</span>
            </div>
          </div>
        </footer>
      </div>
    </div>
  );
};

export default Layout;
