import React, { useState } from 'react';
import { Link, Outlet, useLocation } from 'react-router-dom';

// Simple SVG Icons
const Icons = {
  ChevronLeft: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m15 18-6-6 6-6"/></svg>,
  ChevronRight: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m9 18 6-6-6-6"/></svg>,
  Home: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m3 9 9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/></svg>,
  Wrench: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>,
  CheckSquare: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="9 11 12 14 22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>,
  Brain: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M9.5 2A2.5 2.5 0 0 1 12 4.5v15a2.5 2.5 0 0 1-4.96.44 2.5 2.5 0 0 1-2.96-3.08 3 3 0 0 1-.34-5.58 2.5 2.5 0 0 1 1.32-4.24 2.5 2.5 0 0 1 1.98-3A2.5 2.5 0 0 1 9.5 2Z"/><path d="M14.5 2A2.5 2.5 0 0 0 12 4.5v15a2.5 2.5 0 0 0 4.96.44 2.5 2.5 0 0 0 2.96-3.08 3 3 0 0 0 .34-5.58 2.5 2.5 0 0 0-1.32-4.24 2.5 2.5 0 0 0-1.98-3A2.5 2.5 0 0 0 14.5 2Z"/></svg>,
  Settings: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/></svg>,
  FileText: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M14.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5L14.5 2z"/><polyline points="14 2 14 8 20 8"/><line x1="16" x2="8" y1="13" y2="13"/><line x1="16" x2="8" y1="17" y2="17"/><line x1="10" x2="8" y1="9" y2="9"/></svg>,
  List: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><line x1="8" x2="21" y1="6" y2="6"/><line x1="8" x2="21" y1="12" y2="12"/><line x1="8" x2="21" y1="18" y2="18"/><line x1="3" x2="3.01" y1="6" y2="6"/><line x1="3" x2="3.01" y1="12" y2="12"/><line x1="3" x2="3.01" y1="18" y2="18"/></svg>,
  Play: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polygon points="5 3 19 12 5 21 5 3"/></svg>,
  Terminal: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="4 17 10 11 4 5"/><line x1="12" x2="20" y1="19" y2="19"/></svg>,
  Database: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/><path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/></svg>,
  Activity: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/></svg>,
  Lock: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>,
  Stethoscope: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M4.8 2.3A.3.3 0 1 0 5 2H4a2 2 0 0 0-2 2v5a6 6 0 0 0 6 6v0a6 6 0 0 0 6-6V4a2 2 0 0 0-2-2h-1a.2.2 0 1 0 .3.3"/><path d="M8 15v1a6 6 0 0 0 6 6v0a6 6 0 0 0 6-6v-4"/><circle cx="20" cy="10" r="2"/></svg>,
  AlignLeft: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><line x1="21" x2="3" y1="6" y2="6"/><line x1="15" x2="3" y1="12" y2="12"/><line x1="17" x2="3" y1="18" y2="18"/></svg>,
  Archive: () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect width="20" height="5" x="2" y="3" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8"/><line x1="10" x2="14" y1="12" y2="12"/></svg>,
  Search: () => <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" x2="16.65" y1="21" y2="16.65"/></svg>,
};

const navGroups = [
  {
    title: 'Setup & Config',
    items: [
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
          <Outlet />
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
