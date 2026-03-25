import React, { useMemo, useCallback } from 'react';
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  type Node,
  type Edge,
  type NodeMouseHandler,
  Position,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import type { ApiPrdTask } from '../api/models';
import * as Icons from './icons';

// ---------- Graph algorithms ----------

interface CycleResult {
  hasCycles: boolean;
  cycleEdges: Set<string>;
  cycleNodeIds: Set<string>;
}

function detectCycles(tasks: ApiPrdTask[]): CycleResult {
  const adj = new Map<string, string[]>();
  const ids = new Set(tasks.map(t => t.id));
  for (const t of tasks) {
    adj.set(t.id, (t.dependencies || []).filter(d => ids.has(d)));
  }

  const WHITE = 0, GRAY = 1, BLACK = 2;
  const color = new Map<string, number>();
  const cycleEdges = new Set<string>();
  const cycleNodeIds = new Set<string>();

  for (const id of ids) color.set(id, WHITE);

  function dfs(u: string) {
    color.set(u, GRAY);
    for (const v of adj.get(u) || []) {
      if (color.get(v) === GRAY) {
        cycleEdges.add(`${u}->${v}`);
        cycleNodeIds.add(u);
        cycleNodeIds.add(v);
      } else if (color.get(v) === WHITE) {
        dfs(v);
      }
    }
    color.set(u, BLACK);
  }

  for (const id of ids) {
    if (color.get(id) === WHITE) dfs(id);
  }

  return { hasCycles: cycleEdges.size > 0, cycleEdges, cycleNodeIds };
}

/** Find the critical path (longest chain) via topological order. */
function findCriticalPath(tasks: ApiPrdTask[]): Set<string> {
  const ids = new Set(tasks.map(t => t.id));
  const adj = new Map<string, string[]>(); // task -> dependents (reverse edges for forward pass)
  const deps = new Map<string, string[]>();

  for (const t of tasks) {
    const validDeps = (t.dependencies || []).filter(d => ids.has(d));
    deps.set(t.id, validDeps);
    adj.set(t.id, []);
  }
  for (const t of tasks) {
    for (const d of deps.get(t.id) || []) {
      adj.get(d)?.push(t.id);
    }
  }

  // Topological sort (Kahn's)
  const inDegree = new Map<string, number>();
  for (const id of ids) inDegree.set(id, (deps.get(id) || []).length);

  const queue: string[] = [];
  for (const [id, deg] of inDegree) if (deg === 0) queue.push(id);

  const dist = new Map<string, number>();
  const prev = new Map<string, string | null>();
  for (const id of ids) { dist.set(id, 0); prev.set(id, null); }

  const order: string[] = [];
  while (queue.length > 0) {
    const u = queue.shift()!;
    order.push(u);
    for (const v of adj.get(u) || []) {
      const newDist = dist.get(u)! + 1;
      if (newDist > dist.get(v)!) {
        dist.set(v, newDist);
        prev.set(v, u);
      }
      inDegree.set(v, inDegree.get(v)! - 1);
      if (inDegree.get(v) === 0) queue.push(v);
    }
  }

  // If cycle prevents full topological sort, return empty
  if (order.length !== ids.size) return new Set();

  // Find node with maximum distance
  let maxDist = 0;
  let endNode: string | null = null;
  for (const [id, d] of dist) {
    if (d >= maxDist) { maxDist = d; endNode = id; }
  }

  // Trace back
  const path = new Set<string>();
  let cur = endNode;
  while (cur) {
    path.add(cur);
    cur = prev.get(cur) || null;
  }
  return path;
}

// ---------- Layout (simple layered DAG) ----------

const NODE_W = 220;
const NODE_H = 60;
const H_GAP = 60;
const V_GAP = 100;

function layoutNodes(tasks: ApiPrdTask[]): Map<string, { x: number; y: number }> {
  const ids = new Set(tasks.map(t => t.id));
  const deps = new Map<string, string[]>();
  for (const t of tasks) {
    deps.set(t.id, (t.dependencies || []).filter(d => ids.has(d)));
  }

  // Assign layers via longest path from roots
  const layer = new Map<string, number>();
  const visited = new Set<string>();

  function getLayer(id: string): number {
    if (layer.has(id)) return layer.get(id)!;
    if (visited.has(id)) return 0; // cycle guard
    visited.add(id);
    const parentLayers = (deps.get(id) || []).map(d => getLayer(d) + 1);
    const l = parentLayers.length > 0 ? Math.max(...parentLayers) : 0;
    layer.set(id, l);
    return l;
  }

  for (const id of ids) getLayer(id);

  // Group by layer
  const layers = new Map<number, string[]>();
  for (const [id, l] of layer) {
    if (!layers.has(l)) layers.set(l, []);
    layers.get(l)!.push(id);
  }

  const positions = new Map<string, { x: number; y: number }>();
  for (const [l, nodeIds] of layers) {
    const totalWidth = nodeIds.length * NODE_W + (nodeIds.length - 1) * H_GAP;
    const startX = -totalWidth / 2;
    nodeIds.forEach((id, i) => {
      positions.set(id, {
        x: startX + i * (NODE_W + H_GAP),
        y: l * (NODE_H + V_GAP),
      });
    });
  }

  return positions;
}

// ---------- Node coloring ----------

const STATE_COLORS: Record<string, { bg: string; border: string; text: string }> = {
  todo:       { bg: '#1e293b', border: '#475569', text: '#e2e8f0' },
  active:     { bg: '#1e3a5f', border: '#3b82f6', text: '#93c5fd' },
  in_progress:{ bg: '#1e3a5f', border: '#3b82f6', text: '#93c5fd' },
  merged:     { bg: '#14532d', border: '#22c55e', text: '#86efac' },
  done:       { bg: '#14532d', border: '#22c55e', text: '#86efac' },
  blocked:    { bg: '#451a03', border: '#f97316', text: '#fdba74' },
  failed:     { bg: '#450a0a', border: '#ef4444', text: '#fca5a5' },
};

const DEFAULT_COLOR = { bg: '#1e293b', border: '#475569', text: '#e2e8f0' };

function stateColor(task: ApiPrdTask) {
  const state = (task.metadata?.state as string)?.toLowerCase() || 'todo';
  return STATE_COLORS[state] || DEFAULT_COLOR;
}

// ---------- Component ----------

interface PrdGraphProps {
  tasks: ApiPrdTask[];
  onSelectTask: (taskId: string) => void;
  selectedTaskId: string | null;
}

const PrdGraph: React.FC<PrdGraphProps> = ({ tasks, onSelectTask, selectedTaskId }) => {
  const { nodes, edges, cycles, criticalPath } = useMemo(() => {
    const cyc = detectCycles(tasks);
    const cp = findCriticalPath(tasks);
    const positions = layoutNodes(tasks);

    const ns: Node[] = tasks.map(t => {
      const pos = positions.get(t.id) || { x: 0, y: 0 };
      const colors = stateColor(t);
      const isOnCriticalPath = cp.has(t.id);
      const isInCycle = cyc.cycleNodeIds.has(t.id);
      const isSelected = t.id === selectedTaskId;

      return {
        id: t.id,
        position: pos,
        data: { label: t.id, title: t.title, priority: t.priority },
        sourcePosition: Position.Bottom,
        targetPosition: Position.Top,
        style: {
          width: NODE_W,
          height: NODE_H,
          background: isInCycle ? '#450a0a' : colors.bg,
          border: `2px solid ${isInCycle ? '#ef4444' : isSelected ? '#a78bfa' : isOnCriticalPath ? '#facc15' : colors.border}`,
          borderRadius: '12px',
          color: isInCycle ? '#fca5a5' : colors.text,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          flexDirection: 'column' as const,
          fontSize: '12px',
          fontWeight: isOnCriticalPath ? 700 : 500,
          boxShadow: isSelected ? '0 0 0 2px #a78bfa' : isOnCriticalPath ? '0 0 8px rgba(250,204,21,0.3)' : 'none',
          cursor: 'pointer',
          padding: '8px',
        },
      };
    });

    const ids = new Set(tasks.map(t => t.id));
    const es: Edge[] = [];
    for (const t of tasks) {
      for (const dep of t.dependencies || []) {
        if (!ids.has(dep)) continue;
        const edgeId = `${dep}->${t.id}`;
        const isCycleEdge = cyc.cycleEdges.has(edgeId);
        const isCpEdge = cp.has(dep) && cp.has(t.id);
        es.push({
          id: edgeId,
          source: dep,
          target: t.id,
          animated: isCycleEdge,
          style: {
            stroke: isCycleEdge ? '#ef4444' : isCpEdge ? '#facc15' : '#475569',
            strokeWidth: isCycleEdge || isCpEdge ? 2.5 : 1.5,
          },
        });
      }
    }

    return { nodes: ns, edges: es, cycles: cyc, criticalPath: cp };
  }, [tasks, selectedTaskId]);

  const onNodeClick: NodeMouseHandler = useCallback((_event, node) => {
    onSelectTask(node.id);
  }, [onSelectTask]);

  const nodeLabel = useCallback((node: Node) => {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 2 }}>
        <span style={{ fontWeight: 600, fontSize: 11 }}>{node.data.label as string}</span>
        {node.data.title && (
          <span style={{ fontSize: 10, opacity: 0.7, maxWidth: NODE_W - 20, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {node.data.title as string}
          </span>
        )}
      </div>
    );
  }, []);

  const nodesWithLabels = useMemo(() =>
    nodes.map(n => ({ ...n, data: { ...n.data, label: nodeLabel(n) } })),
    [nodes, nodeLabel]
  );

  return (
    <div className="relative h-full w-full">
      {cycles.hasCycles && (
        <div className="absolute left-4 top-4 z-10 flex items-center gap-2 rounded-xl border border-rose-500/50 bg-rose-500/10 px-4 py-2 text-sm text-rose-400 backdrop-blur-sm">
          <Icons.AlertTriangleIcon className="h-4 w-4 flex-shrink-0" />
          <span>Circular dependencies detected — highlighted in red</span>
        </div>
      )}
      {criticalPath.size > 1 && (
        <div className="absolute right-4 top-4 z-10 flex items-center gap-2 rounded-xl border border-yellow-500/50 bg-yellow-500/10 px-4 py-2 text-sm text-yellow-400 backdrop-blur-sm">
          <Icons.ActivityIcon className="h-4 w-4 flex-shrink-0" />
          <span>Critical path: {criticalPath.size} tasks</span>
        </div>
      )}
      <ReactFlow
        nodes={nodesWithLabels}
        edges={edges}
        onNodeClick={onNodeClick}
        fitView
        fitViewOptions={{ padding: 0.3 }}
        minZoom={0.1}
        maxZoom={2}
        proOptions={{ hideAttribution: true }}
      >
        <Background color="#334155" gap={20} />
        <Controls
          showInteractive={false}
          style={{
            background: 'var(--bg-secondary)',
            border: '1px solid var(--border)',
            borderRadius: '12px',
            overflow: 'hidden',
          }}
        />
        <MiniMap
          nodeColor={(n) => {
            const style = n.style as Record<string, string> | undefined;
            return style?.background || '#475569';
          }}
          maskColor="rgba(0,0,0,0.6)"
          style={{
            background: 'var(--bg-secondary)',
            border: '1px solid var(--border)',
            borderRadius: '12px',
          }}
        />
      </ReactFlow>
    </div>
  );
};

export default PrdGraph;
