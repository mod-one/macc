import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Link, useLocation } from 'react-router-dom';
import { Icons } from './NavIcons';
import GitGraphView from './GitGraphView';

const PANEL_COLLAPSED_KEY = 'macc.gitGraphPanel.collapsed';
const PANEL_WIDTH_KEY = 'macc.gitGraphPanel.width';
const MIN_PANEL_WIDTH = 280;
const MAX_PANEL_WIDTH = 620;
const DEFAULT_PANEL_WIDTH = 350;

function readCollapsedState(): boolean {
  const raw = window.localStorage.getItem(PANEL_COLLAPSED_KEY);
  return raw === '1';
}

function readPanelWidth(): number {
  const raw = window.localStorage.getItem(PANEL_WIDTH_KEY);
  const parsed = raw ? Number.parseInt(raw, 10) : Number.NaN;
  if (!Number.isFinite(parsed)) {
    return DEFAULT_PANEL_WIDTH;
  }
  return Math.max(MIN_PANEL_WIDTH, Math.min(MAX_PANEL_WIDTH, parsed));
}

const GitGraphPanel: React.FC = () => {
  const location = useLocation();
  const [collapsed, setCollapsed] = useState(false);
  const [panelWidth, setPanelWidth] = useState(DEFAULT_PANEL_WIDTH);
  const [resizing, setResizing] = useState(false);
  const dragStartX = useRef(0);
  const dragStartWidth = useRef(DEFAULT_PANEL_WIDTH);

  useEffect(() => {
    setCollapsed(readCollapsedState());
    setPanelWidth(readPanelWidth());
  }, []);

  useEffect(() => {
    window.localStorage.setItem(PANEL_COLLAPSED_KEY, collapsed ? '1' : '0');
  }, [collapsed]);

  useEffect(() => {
    window.localStorage.setItem(PANEL_WIDTH_KEY, String(panelWidth));
  }, [panelWidth]);

  useEffect(() => {
    if (!resizing) {
      return undefined;
    }

    const handleMove = (event: MouseEvent) => {
      const delta = dragStartX.current - event.clientX;
      const next = Math.max(
        MIN_PANEL_WIDTH,
        Math.min(MAX_PANEL_WIDTH, dragStartWidth.current + delta),
      );
      setPanelWidth(next);
    };

    const handleUp = () => {
      setResizing(false);
    };

    window.addEventListener('mousemove', handleMove);
    window.addEventListener('mouseup', handleUp);

    return () => {
      window.removeEventListener('mousemove', handleMove);
      window.removeEventListener('mouseup', handleUp);
    };
  }, [resizing]);

  const panelStyle = useMemo(
    () => ({
      width: collapsed ? 46 : panelWidth,
      minWidth: collapsed ? 46 : panelWidth,
      maxWidth: collapsed ? 46 : panelWidth,
    }),
    [collapsed, panelWidth],
  );

  return (
    <aside
      className="relative flex h-full shrink-0 flex-col border-l border-[var(--border)] bg-[var(--bg-secondary)]"
      style={panelStyle}
    >
      {!collapsed && (
        <button
          type="button"
          className="absolute left-0 top-0 h-full w-1 cursor-col-resize bg-transparent hover:bg-[var(--accent)]/30"
          aria-label="Resize git graph panel"
          onMouseDown={(event) => {
            dragStartX.current = event.clientX;
            dragStartWidth.current = panelWidth;
            setResizing(true);
          }}
        />
      )}

      <header className="flex h-11 items-center justify-between gap-2 border-b border-[var(--border)] px-2">
        {collapsed ? (
          <button
            type="button"
            onClick={() => setCollapsed(false)}
            className="mx-auto rounded border border-[var(--border)] p-1 text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
            title="Expand Git Graph panel"
          >
            <Icons.ChevronLeft />
          </button>
        ) : (
          <>
            <div className="truncate pl-2 text-xs font-semibold uppercase tracking-wider text-[var(--text-secondary)]">
              Git Graph
            </div>
            <div className="flex items-center gap-1">
              <Link
                to="/ops/git"
                state={{ from: location.pathname }}
                className="rounded border border-[var(--border)] px-2 py-1 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
                title="Open full-page git graph"
              >
                Full Page
              </Link>
              <button
                type="button"
                onClick={() => setCollapsed(true)}
                className="rounded border border-[var(--border)] p-1 text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
                title="Collapse Git Graph panel"
              >
                <Icons.ChevronRight />
              </button>
            </div>
          </>
        )}
      </header>

      {!collapsed && (
        <div className="min-h-0 flex-1 p-2">
          <GitGraphView mode="panel" />
        </div>
      )}
    </aside>
  );
};

export default GitGraphPanel;
