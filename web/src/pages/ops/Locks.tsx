import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Background,
  Controls,
  Handle,
  MarkerType,
  MiniMap,
  ReactFlow,
  Position,
  type NodeProps,
  useReactFlow,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { getRegistryTasks, getWorktrees } from '../../api/client';
import type { ApiRegistryTask, ApiWorktree } from '../../api/models';
import { Button } from '../../components/Button';
import { ErrorBanner } from '../../components/ErrorBanner';
import { LoadingSpinner } from '../../components/LoadingSpinner';
import { RightDrawer } from '../../components/RightDrawer';
import { StatusBadge } from '../../components/StatusBadge';
import * as Icons from '../../components/icons';
import { cn, surfaceClassName } from '../../components/styles';
import { useCoordinatorStore } from '../../store';
import { buildLocksGraph, type LocksNode, type LocksNodeData, type LocksNodeKind } from './locksGraph';

interface Selection {
  id: string;
  kind: LocksNodeKind;
}

interface GraphSummaryCardProps {
  label: string;
  value: string | number;
  tone: 'accent' | 'warning' | 'danger' | 'muted';
  icon: React.ReactNode;
}

const SUMMARY_TONES: Record<GraphSummaryCardProps['tone'], string> = {
  accent: 'text-cyan-300',
  warning: 'text-amber-300',
  danger: 'text-rose-300',
  muted: 'text-slate-300',
};

function GraphSummaryCard({ label, value, tone, icon }: GraphSummaryCardProps) {
  return (
    <article className={cn(surfaceClassName, 'p-5')}>
      <div className="flex items-start justify-between gap-4">
        <div className="space-y-1">
          <p className="text-sm font-medium text-[var(--text-secondary)]">{label}</p>
          <p className={cn('text-3xl font-semibold tracking-tight', SUMMARY_TONES[tone])}>{value}</p>
        </div>
        <div className="rounded-xl border border-white/10 bg-white/5 p-3 text-[var(--accent)]">{icon}</div>
      </div>
    </article>
  );
}

function kindLabel(kind: LocksNodeKind): string {
  switch (kind) {
    case 'task':
      return 'Task';
    case 'resource':
      return 'Resource';
    case 'worktree':
      return 'Worktree';
    case 'session':
      return 'Session';
    default:
      return kind;
  }
}

function kindIcon(kind: LocksNodeKind): React.ReactNode {
  switch (kind) {
    case 'task':
      return <Icons.ActivityIcon className="h-4 w-4" />;
    case 'resource':
      return <Icons.FolderIcon className="h-4 w-4" />;
    case 'worktree':
      return <Icons.FolderOpenIcon className="h-4 w-4" />;
    case 'session':
      return <Icons.ClockIcon className="h-4 w-4" />;
    default:
      return null;
  }
}

function statusTone(status: string | null): 'active' | 'blocked' | 'failed' | 'merged' | 'paused' | 'todo' {
  const normalized = (status ?? '').toLowerCase();
  if (normalized === 'active' || normalized === 'in_progress') return 'active';
  if (normalized === 'blocked') return 'blocked';
  if (normalized === 'failed' || normalized === 'error') return 'failed';
  if (normalized === 'merged' || normalized === 'done' || normalized === 'complete') return 'merged';
  if (normalized === 'paused') return 'paused';
  return 'todo';
}

function LocksNodeFrame({ data }: NodeProps<LocksNode>) {
  const isTask = data.kind === 'task';
  const isResource = data.kind === 'resource';
  const isWorktree = data.kind === 'worktree';
  const isSession = data.kind === 'session';

  const baseStyles: React.CSSProperties = {
    width: 250,
    minHeight: 88,
    borderColor: data.accent,
    background: 'linear-gradient(180deg, color-mix(in srgb, var(--bg-secondary) 92%, transparent), color-mix(in srgb, var(--bg-secondary) 82%, transparent))',
    color: 'var(--text-primary)',
    boxShadow: `0 0 0 1px color-mix(in srgb, ${data.accent} 25%, transparent), 0 18px 35px rgba(0, 0, 0, 0.2)`,
  };

  if (isResource) {
    baseStyles.clipPath = 'polygon(25% 6%, 75% 6%, 100% 50%, 75% 94%, 25% 94%, 0% 50%)';
  } else if (isSession) {
    baseStyles.borderRadius = '999px';
    baseStyles.borderStyle = 'dashed';
  } else if (isWorktree) {
    baseStyles.borderRadius = '22px';
  } else if (isTask) {
    baseStyles.borderRadius = '18px';
  }

  return (
    <div
      className={cn(
        'relative flex h-full w-full items-stretch justify-stretch border px-4 py-3 text-sm',
        isResource && 'px-6 py-5',
      )}
      style={baseStyles}
    >
      <Handle id="in" position={Position.Left} style={{ opacity: 0 }} type="target" />
      <Handle id="out" position={Position.Right} style={{ opacity: 0 }} type="source" />
      <div className="flex w-full flex-col gap-2">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0 space-y-1">
            <div className="flex items-center gap-2">
              <span className="inline-flex h-6 w-6 items-center justify-center rounded-full border border-white/10 bg-white/5 text-[var(--text-primary)]">
                {kindIcon(data.kind)}
              </span>
              <p className="truncate font-semibold">{data.title}</p>
            </div>
            <p className="text-xs uppercase tracking-[0.2em] text-[var(--text-secondary)]">
              {kindLabel(data.kind)}
              {data.subtitle ? ` • ${data.subtitle}` : ''}
            </p>
          </div>
          {data.badge && (
            <span className="shrink-0 rounded-full border border-white/10 bg-white/5 px-2 py-1 text-[10px] font-semibold uppercase tracking-[0.16em] text-[var(--text-secondary)]">
              {data.badge}
            </span>
          )}
        </div>

        {data.status && (
          <StatusBadge status={data.status} tone={statusTone(data.status)} className="w-fit" />
        )}

        {data.count !== null && (
          <p className="text-xs text-[var(--text-secondary)]">
            {data.kind === 'task'
              ? `${data.count} dependency${data.count === 1 ? '' : 'ies'}`
              : `${data.count} related item${data.count === 1 ? '' : 's'}`}
          </p>
        )}

        <div className="space-y-1">
          {data.summary.slice(0, 2).map((line) => (
            <p key={line} className="line-clamp-1 text-xs text-[var(--text-secondary)]">
              {line}
            </p>
          ))}
        </div>
      </div>
    </div>
  );
}

function GraphToolbar({ onRefresh, refreshing }: { onRefresh: () => void; refreshing: boolean }) {
  const { fitView } = useReactFlow();

  return (
    <div className="absolute right-4 top-4 z-20 flex flex-wrap items-center gap-2">
      <Button
        className="h-9 gap-2 border-white/10 bg-white/5 px-3 text-xs hover:bg-white/10"
        onClick={() => fitView({ padding: 0.24, duration: 250 })}
        type="button"
      >
        <Icons.SwitchIcon className="h-3.5 w-3.5" />
        Reset view
      </Button>
      <Button
        className="h-9 gap-2 border-white/10 bg-white/5 px-3 text-xs hover:bg-white/10"
        onClick={onRefresh}
        type="button"
      >
        <Icons.RefreshIcon className={cn('h-3.5 w-3.5', refreshing && 'animate-spin')} />
        Refresh
      </Button>
    </div>
  );
}

const nodeTypes = {
  task: LocksNodeFrame,
  resource: LocksNodeFrame,
  worktree: LocksNodeFrame,
  session: LocksNodeFrame,
};

const Locks: React.FC = () => {
  const [tasks, setTasks] = useState<ApiRegistryTask[]>([]);
  const [worktrees, setWorktrees] = useState<ApiWorktree[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedNode, setSelectedNode] = useState<Selection | null>(null);
  const [isDrawerPinned, setIsDrawerPinned] = useState(false);
  const runAction = useCoordinatorStore((state) => state.runAction);
  const pendingAction = useCoordinatorStore((state) => state.pendingAction);

  const refresh = useCallback(async () => {
    setIsLoading(true);
    try {
      const [nextTasks, nextWorktrees] = await Promise.all([getRegistryTasks(), getWorktrees()]);
      setTasks(nextTasks);
      setWorktrees(nextWorktrees);
      setError(null);
    } catch (err) {
      console.error('Failed to fetch lock graph data:', err);
      setError(err instanceof Error ? err.message : 'Failed to load lock graph.');
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const interval = window.setInterval(() => {
      void refresh();
    }, 15_000);

    return () => window.clearInterval(interval);
  }, [refresh]);

  const graph = useMemo(() => buildLocksGraph(tasks, worktrees), [tasks, worktrees]);

  const selectedNodeData = useMemo(() => {
    if (!selectedNode) {
      return null;
    }

    if (selectedNode.kind === 'task') {
      return tasks.find((task) => task.id === selectedNode.id) ?? null;
    }

    if (selectedNode.kind === 'resource') {
      return {
        id: selectedNode.id.replace(/^resource:/, ''),
        contenders: graph.resourceContenders.get(selectedNode.id.replace(/^resource:/, '')) ?? [],
        owner: graph.resourceOwners.get(selectedNode.id.replace(/^resource:/, '')) ?? null,
      };
    }

    if (selectedNode.kind === 'worktree') {
      return graph.worktreeByNodeId.get(selectedNode.id) ?? null;
    }

    return graph.sessionByNodeId.get(selectedNode.id) ?? null;
  }, [graph.resourceContenders, graph.resourceOwners, graph.sessionByNodeId, graph.worktreeByNodeId, selectedNode, tasks]);

  const deadlockCount = graph.cycleNodeIds.size > 0 ? 1 : 0;
  const selectedTaskIds = Array.from(graph.cycleNodeIds).filter((id) => tasks.some((task) => task.id === id));

  const handleCoordinatorAction = useCallback(async (action: 'reconcile' | 'unlock' | 'cleanup') => {
    try {
      await runAction(action);
      await refresh();
    } catch (err) {
      console.error(`Coordinator action ${action} failed:`, err);
    }
  }, [refresh, runAction]);

  const showDeadlockBanner = graph.cycleNodeIds.size > 0;

  const selectedTask = selectedNode?.kind === 'task'
    ? tasks.find((task) => task.id === selectedNode.id) ?? null
    : null;

  const selectedResource = selectedNode?.kind === 'resource'
    ? {
        id: selectedNode.id.replace(/^resource:/, ''),
        contenders: graph.resourceContenders.get(selectedNode.id.replace(/^resource:/, '')) ?? [],
        owner: graph.resourceOwners.get(selectedNode.id.replace(/^resource:/, '')) ?? null,
      }
    : null;

  const selectedWorktree = selectedNode?.kind === 'worktree'
    ? graph.worktreeByNodeId.get(selectedNode.id) ?? null
    : null;

  const selectedSession = selectedNode?.kind === 'session'
    ? graph.sessionByNodeId.get(selectedNode.id) ?? null
    : null;

  return (
    <section className="relative flex min-h-[calc(100vh-80px)] flex-col gap-6 overflow-hidden pb-12">
      <div className="absolute inset-x-0 top-0 -z-10 h-72 bg-[radial-gradient(circle_at_top,_rgba(56,189,248,0.18),_transparent_55%),radial-gradient(circle_at_left,_rgba(244,114,182,0.14),_transparent_40%),linear-gradient(180deg,_rgba(15,23,42,0.98),_rgba(15,23,42,0.85))]" />

      <header className={cn(surfaceClassName, 'relative overflow-hidden p-6')}>
        <div className="absolute inset-0 bg-[radial-gradient(circle_at_top_right,_rgba(56,189,248,0.12),_transparent_35%),radial-gradient(circle_at_bottom_left,_rgba(250,204,21,0.08),_transparent_30%)]" />
        <div className="relative flex flex-col gap-6">
          <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
            <div className="space-y-3">
              <div className="inline-flex items-center gap-2 rounded-full border border-cyan-400/20 bg-cyan-400/10 px-3 py-1 text-xs font-semibold uppercase tracking-[0.2em] text-cyan-200">
                <Icons.ActivityIcon className="h-3.5 w-3.5" />
                Dependency Graph & Locks
              </div>
              <div>
                <h1 className="text-4xl font-semibold tracking-tight text-[var(--text-primary)]">
                  Task dependencies, resource contention, and leases
                </h1>
                <p className="mt-2 max-w-3xl text-sm leading-6 text-[var(--text-secondary)]">
                  Inspect dependency edges, exclusive resource conflicts, tool session leases, and worktree slot allocation from one graph.
                </p>
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-3">
              <Button
                className="h-10 gap-2 border-white/10 bg-white/5 px-4 hover:bg-white/10"
                onClick={() => void refresh()}
                disabled={isLoading}
                type="button"
              >
                <Icons.RefreshIcon className={cn('h-4 w-4', isLoading && 'animate-spin')} />
                Refresh
              </Button>
              <Button
                className="h-10 gap-2 border-white/10 bg-white/5 px-4 hover:bg-white/10"
                onClick={() => setSelectedNode(null)}
                type="button"
              >
                <Icons.XCircleIcon className="h-4 w-4" />
                Clear selection
              </Button>
            </div>
          </div>

          <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
            <GraphSummaryCard
              label="Tasks"
              value={tasks.length}
              tone="accent"
              icon={<Icons.ActivityIcon className="h-5 w-5" />}
            />
            <GraphSummaryCard
              label="Exclusive resources"
              value={graph.resourceContenders.size}
              tone="warning"
              icon={<Icons.FolderIcon className="h-5 w-5" />}
            />
            <GraphSummaryCard
              label="Worktree slots"
              value={worktrees.length}
              tone="muted"
              icon={<Icons.FolderOpenIcon className="h-5 w-5" />}
            />
            <GraphSummaryCard
              label="Deadlock cycles"
              value={deadlockCount}
              tone={deadlockCount > 0 ? 'danger' : 'accent'}
              icon={<Icons.AlertTriangleIcon className="h-5 w-5" />}
            />
          </div>
        </div>
      </header>

      {error && (
        <ErrorBanner
          message={error}
          title="Unable to load lock graph"
          onRetry={() => void refresh()}
        />
      )}

      {showDeadlockBanner && (
        <section
          className="rounded-[var(--radius-card)] border border-rose-500/30 bg-rose-500/10 px-5 py-4 text-[var(--text-primary)] shadow-[0_0_0_1px_rgba(244,63,94,0.12)]"
          role="status"
          aria-live="polite"
        >
          <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
            <div className="space-y-1">
              <div className="flex items-center gap-2 text-rose-300">
                <Icons.AlertTriangleIcon className="h-4 w-4" />
                <span className="text-sm font-semibold uppercase tracking-[0.18em]">Deadlock warning</span>
              </div>
              <p className="text-sm text-[var(--text-secondary)]">
                {selectedTaskIds.length > 0
                  ? `Detected a cycle across ${selectedTaskIds.length} task${selectedTaskIds.length === 1 ? '' : 's'} and shared resources.`
                  : 'Detected a cycle in the dependency and resource graph.'}
              </p>
            </div>

            <div className="flex flex-wrap gap-2">
              <Button
                className="h-10 gap-2 border-transparent bg-rose-600 px-4 text-white hover:bg-rose-500"
                onClick={() => void handleCoordinatorAction('reconcile')}
                disabled={pendingAction !== null}
                type="button"
              >
                Reconcile
              </Button>
              <Button
                className="h-10 gap-2 border-transparent bg-amber-500 px-4 text-slate-950 hover:bg-amber-400"
                onClick={() => void handleCoordinatorAction('unlock')}
                disabled={pendingAction !== null}
                type="button"
              >
                Unlock
              </Button>
              <Button
                className="h-10 gap-2 border-transparent bg-cyan-500 px-4 text-slate-950 hover:bg-cyan-400"
                onClick={() => void handleCoordinatorAction('cleanup')}
                disabled={pendingAction !== null}
                type="button"
              >
                Cleanup
              </Button>
            </div>
          </div>
        </section>
      )}

      <section className={cn(surfaceClassName, 'relative min-h-[36rem] overflow-hidden p-2')}>
        {isLoading && tasks.length === 0 ? (
          <div className="flex min-h-[36rem] items-center justify-center">
            <LoadingSpinner label="Loading dependency graph" size="lg" />
          </div>
        ) : (
          <div className="relative h-[42rem]">
            <ReactFlow
              nodes={graph.nodes}
              edges={graph.edges}
              nodeTypes={nodeTypes}
              onNodeClick={(_event, node) => {
                setSelectedNode({ id: node.id, kind: node.type as LocksNodeKind });
                setIsDrawerPinned(false);
              }}
              fitView
              fitViewOptions={{ padding: 0.18, minZoom: 0.2 }}
              minZoom={0.18}
              maxZoom={2.25}
              nodesDraggable={false}
              elementsSelectable
              panOnDrag
              zoomOnScroll
              zoomOnPinch
              defaultEdgeOptions={{
                markerEnd: { type: MarkerType.ArrowClosed, width: 18, height: 18 },
              }}
            >
              <GraphToolbar
                onRefresh={() => void refresh()}
                refreshing={isLoading}
              />
              <Background color="rgba(148,163,184,0.3)" gap={22} />
              <Controls
                showInteractive={false}
                style={{
                  background: 'var(--bg-secondary)',
                  border: '1px solid var(--border)',
                  borderRadius: '14px',
                  overflow: 'hidden',
                }}
              />
              <MiniMap
                nodeColor={(node) => {
                  const data = node.data as unknown as LocksNodeData | undefined;
                  return data?.accent ?? '#64748b';
                }}
                maskColor="rgba(2,6,23,0.7)"
                style={{
                  background: 'var(--bg-secondary)',
                  border: '1px solid var(--border)',
                  borderRadius: '14px',
                }}
              />
            </ReactFlow>

            <div className="pointer-events-none absolute bottom-4 left-4 z-10 rounded-2xl border border-white/10 bg-slate-950/60 px-4 py-3 text-xs text-[var(--text-secondary)] shadow-2xl backdrop-blur">
              <div className="mb-2 font-semibold uppercase tracking-[0.18em] text-[var(--text-primary)]">Legend</div>
              <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
                <LegendItem icon={<Icons.ActivityIcon className="h-3.5 w-3.5" />} label="Task node" swatch="bg-cyan-400" />
                <LegendItem icon={<Icons.FolderIcon className="h-3.5 w-3.5" />} label="Resource node" swatch="bg-amber-400" />
                <LegendItem icon={<Icons.FolderOpenIcon className="h-3.5 w-3.5" />} label="Worktree slot" swatch="bg-violet-400" />
                <LegendItem icon={<Icons.ClockIcon className="h-3.5 w-3.5" />} label="Session lease" swatch="bg-teal-400" />
              </div>
            </div>
          </div>
        )}
      </section>

      <RightDrawer
        open={selectedNode !== null || isDrawerPinned}
        onOpenChange={(open) => {
          if (!open) {
            setSelectedNode(null);
            setIsDrawerPinned(false);
          }
        }}
        pinned={isDrawerPinned}
        onPinnedChange={setIsDrawerPinned}
        title={selectedNode ? `${kindLabel(selectedNode.kind)} details` : 'Selection details'}
        description={selectedNode ? selectedNode.id : 'Select a node in the graph to inspect its relationships.'}
      >
        {!selectedNodeData ? (
          <div className="space-y-3 text-sm text-[var(--text-secondary)]">
            <p>Choose a task, resource, worktree slot, or session node to inspect its details.</p>
            <p>The drawer remains pinned while you continue exploring the graph.</p>
          </div>
        ) : selectedNode?.kind === 'task' && selectedTask ? (
          <div className="space-y-5">
            <div className="space-y-2">
              <StatusBadge status={selectedTask.state} tone={statusTone(selectedTask.state)} className="w-fit" />
              <h2 className="text-lg font-semibold">{selectedTask.title ?? selectedTask.id}</h2>
              <p className="text-sm text-[var(--text-secondary)]">{selectedTask.description ?? selectedTask.objective ?? 'No description available.'}</p>
            </div>

            <DetailBlock title="Dependencies" items={selectedTask.dependencies} />
            <DetailBlock title="Exclusive resources" items={selectedTask.exclusiveResources} />
            <DetailBlock title="Steps" items={selectedTask.steps} />

            <div className="rounded-2xl border border-white/10 bg-white/5 p-4">
              <p className="text-xs font-semibold uppercase tracking-[0.2em] text-[var(--text-secondary)]">Worktree lease</p>
              <dl className="mt-3 space-y-2 text-sm">
                <Row label="Path" value={selectedTask.worktree?.worktreePath ?? 'None'} />
                <Row label="Branch" value={selectedTask.worktree?.branch ?? 'None'} />
                <Row label="Session" value={selectedTask.worktree?.sessionId ?? 'None'} />
                <Row label="Base" value={selectedTask.worktree?.baseBranch ?? 'None'} />
              </dl>
            </div>
          </div>
        ) : selectedNode?.kind === 'resource' && selectedResource ? (
          <div className="space-y-5">
            <div className="rounded-2xl border border-amber-400/20 bg-amber-400/10 p-4">
              <h2 className="text-lg font-semibold">{selectedResource.id}</h2>
              <p className="mt-1 text-sm text-[var(--text-secondary)]">
                Shared exclusive resource contested by {selectedResource.contenders.length} task{selectedResource.contenders.length === 1 ? '' : 's'}.
              </p>
            </div>

            <DetailBlock title="Contending tasks" items={selectedResource.contenders} />
            <Row label="Owner" value={selectedResource.owner ?? 'Unassigned'} />
          </div>
        ) : selectedNode?.kind === 'worktree' && selectedWorktree ? (
          <div className="space-y-5">
            <div className="space-y-2">
              <StatusBadge status={selectedWorktree.status ?? 'unknown'} tone={statusTone(selectedWorktree.status)} className="w-fit" />
              <h2 className="text-lg font-semibold">{selectedWorktree.slug ?? selectedWorktree.id}</h2>
              <p className="text-sm text-[var(--text-secondary)]">{selectedWorktree.path}</p>
            </div>

            <div className="rounded-2xl border border-violet-400/20 bg-violet-400/10 p-4">
              <dl className="space-y-2 text-sm">
                <Row label="Branch" value={selectedWorktree.branch ?? 'None'} />
                <Row label="Tool" value={selectedWorktree.tool ?? 'None'} />
                <Row label="Locked" value={selectedWorktree.locked ? 'Yes' : 'No'} />
                <Row label="Prunable" value={selectedWorktree.prunable ? 'Yes' : 'No'} />
                <Row label="Session label" value={selectedWorktree.sessionLabel ?? 'None'} />
              </dl>
            </div>
          </div>
        ) : selectedNode?.kind === 'session' && selectedSession ? (
          <div className="space-y-5">
            <div className="rounded-2xl border border-teal-400/20 bg-teal-400/10 p-4">
              <h2 className="text-lg font-semibold">{selectedSession.label}</h2>
              <p className="mt-1 text-sm text-[var(--text-secondary)]">Tool session lease and worktree allocation.</p>
            </div>

            <DetailBlock title="Tasks" items={selectedSession.tasks} />
            <DetailBlock title="Worktrees" items={selectedSession.worktrees} />
          </div>
        ) : (
          <div className="space-y-3 text-sm text-[var(--text-secondary)]">
            <p>The selected node could not be resolved against the current snapshot.</p>
          </div>
        )}
      </RightDrawer>
    </section>
  );
};

function LegendItem({ icon, label, swatch }: { icon: React.ReactNode; label: string; swatch: string }) {
  return (
    <div className="flex items-center gap-2">
      <span className={cn('inline-flex h-5 w-5 items-center justify-center rounded-full', swatch)}>{icon}</span>
      <span>{label}</span>
    </div>
  );
}

function DetailBlock({ title, items }: { title: string; items: string[] }) {
  if (items.length === 0) {
    return (
      <div className="rounded-2xl border border-white/10 bg-white/5 p-4">
        <p className="text-xs font-semibold uppercase tracking-[0.2em] text-[var(--text-secondary)]">{title}</p>
        <p className="mt-2 text-sm text-[var(--text-secondary)]">None</p>
      </div>
    );
  }

  return (
    <div className="rounded-2xl border border-white/10 bg-white/5 p-4">
      <p className="text-xs font-semibold uppercase tracking-[0.2em] text-[var(--text-secondary)]">{title}</p>
      <ul className="mt-3 space-y-2 text-sm text-[var(--text-primary)]">
        {items.map((item) => (
          <li key={item} className="rounded-xl border border-white/8 bg-black/20 px-3 py-2">
            {item}
          </li>
        ))}
      </ul>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-start justify-between gap-4">
      <dt className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[var(--text-secondary)]">{label}</dt>
      <dd className="max-w-[65%] text-right text-sm text-[var(--text-primary)]">{value}</dd>
    </div>
  );
}

export default Locks;
