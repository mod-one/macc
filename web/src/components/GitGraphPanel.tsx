import React, { Suspense, lazy, useEffect, useMemo, useRef, useState } from 'react';
import { Link, useLocation } from 'react-router-dom';
import { Icons } from './NavIcons';

class GitGraphErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { crashed: boolean; message: string }
> {
  constructor(props: { children: React.ReactNode }) {
    super(props);
    this.state = { crashed: false, message: '' };
  }
  static getDerivedStateFromError(error: unknown) {
    return {
      crashed: true,
      message: error instanceof Error ? error.message : 'Unknown error',
    };
  }
  render() {
    if (this.state.crashed) {
      return (
        <div className="flex h-full items-center justify-center p-3 text-center text-xs text-[var(--text-muted)]">
          Git graph unavailable
          <br />
          <span className="text-[var(--error)] mt-1 block font-mono">{this.state.message}</span>
        </div>
      );
    }
    return this.props.children;
  }
}

const GitGraphView = lazy(() => import('./GitGraphView'));

const PANEL_COLLAPSED_KEY = 'macc.gitGraphPanel.collapsed';
const PANEL_WIDTH_KEY = 'macc.gitGraphPanel.width';
const MIN_PANEL_WIDTH = 280;
const MAX_PANEL_WIDTH = 620;
const DEFAULT_PANEL_WIDTH = 350;
const RESIZE_STEP = 24;

function readCollapsedState(): boolean {
  if (typeof window === 'undefined') {
    return false;
  }
  const raw = window.localStorage.getItem(PANEL_COLLAPSED_KEY);
  return raw === '1';
}

function readPanelWidth(): number {
  if (typeof window === 'undefined') {
    return DEFAULT_PANEL_WIDTH;
  }
  const raw = window.localStorage.getItem(PANEL_WIDTH_KEY);
  const parsed = raw ? Number.parseInt(raw, 10) : Number.NaN;
  if (!Number.isFinite(parsed)) {
    return DEFAULT_PANEL_WIDTH;
  }
  return Math.max(MIN_PANEL_WIDTH, Math.min(MAX_PANEL_WIDTH, parsed));
}

const GitGraphPanel: React.FC = () => {
  const location = useLocation();
  const [collapsed, setCollapsed] = useState(readCollapsedState);
  const [panelWidth, setPanelWidth] = useState(readPanelWidth);
  const [resizing, setResizing] = useState(false);
  const dragStartX = useRef(0);
  const dragStartWidth = useRef(DEFAULT_PANEL_WIDTH);

  const clampWidth = (value: number) => Math.max(MIN_PANEL_WIDTH, Math.min(MAX_PANEL_WIDTH, value));

  const updatePanelWidth = (delta: number) => {
    setPanelWidth((current) => clampWidth(current + delta));
  };

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
        <div
          aria-label="Resize git graph panel"
          aria-orientation="vertical"
          aria-valuemax={MAX_PANEL_WIDTH}
          aria-valuemin={MIN_PANEL_WIDTH}
          aria-valuenow={panelWidth}
          aria-valuetext={`${panelWidth} pixels`}
          className="absolute left-0 top-0 h-full w-1 cursor-col-resize bg-transparent hover:bg-[var(--accent)]/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-inset"
          onKeyDown={(event) => {
            if (event.key === 'ArrowLeft' || event.key === 'ArrowDown') {
              event.preventDefault();
              updatePanelWidth(-RESIZE_STEP);
            } else if (event.key === 'ArrowRight' || event.key === 'ArrowUp') {
              event.preventDefault();
              updatePanelWidth(RESIZE_STEP);
            } else if (event.key === 'Home') {
              event.preventDefault();
              setPanelWidth(MIN_PANEL_WIDTH);
            } else if (event.key === 'End') {
              event.preventDefault();
              setPanelWidth(MAX_PANEL_WIDTH);
            }
          }}
          onMouseDown={(event) => {
            event.preventDefault();
            dragStartX.current = event.clientX;
            dragStartWidth.current = panelWidth;
            setResizing(true);
          }}
          role="separator"
          tabIndex={0}
        />
      )}

      <header className="flex h-11 items-center justify-between gap-2 border-b border-[var(--border)] px-2">
        {collapsed ? (
          <button
            type="button"
            onClick={() => setCollapsed(false)}
            className="mx-auto rounded border border-[var(--border)] p-1 text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
            aria-label="Expand Git Graph panel"
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
                aria-label="Collapse Git Graph panel"
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
          <GitGraphErrorBoundary>
            <Suspense
              fallback={
                <div className="flex h-full items-center justify-center text-sm text-[var(--text-muted)]">
                  Loading git graph...
                </div>
              }
            >
              <GitGraphView mode="panel" />
            </Suspense>
          </GitGraphErrorBoundary>
        </div>
      )}
    </aside>
  );
};

export default GitGraphPanel;
