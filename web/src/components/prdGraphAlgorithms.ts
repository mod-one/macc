import type { ApiPrdTask } from '../api/models';

// ---------- Cycle detection (DFS coloring) ----------

export interface CycleResult {
  hasCycles: boolean;
  cycleEdges: Set<string>;
  cycleNodeIds: Set<string>;
}

export function detectCycles(tasks: ApiPrdTask[]): CycleResult {
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

// ---------- Critical path (longest chain via Kahn's topological sort) ----------

export function findCriticalPath(tasks: ApiPrdTask[]): Set<string> {
  const ids = new Set(tasks.map(t => t.id));
  const adj = new Map<string, string[]>(); // task -> dependents (forward edges)
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
  let queueIdx = 0; // index-based queue to avoid O(n) shift
  while (queueIdx < queue.length) {
    const u = queue[queueIdx++];
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

// ---------- DAG layout ----------

const NODE_W = 220;
const NODE_H = 60;
const H_GAP = 60;
const V_GAP = 100;

export { NODE_W, NODE_H };

export function layoutNodes(tasks: ApiPrdTask[]): Map<string, { x: number; y: number }> {
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
  todo:        { bg: '#1e293b', border: '#475569', text: '#e2e8f0' },
  active:      { bg: '#1e3a5f', border: '#3b82f6', text: '#93c5fd' },
  in_progress: { bg: '#1e3a5f', border: '#3b82f6', text: '#93c5fd' },
  merged:      { bg: '#14532d', border: '#22c55e', text: '#86efac' },
  done:        { bg: '#14532d', border: '#22c55e', text: '#86efac' },
  blocked:     { bg: '#451a03', border: '#f97316', text: '#fdba74' },
  failed:      { bg: '#450a0a', border: '#ef4444', text: '#fca5a5' },
};

const DEFAULT_COLOR = { bg: '#1e293b', border: '#475569', text: '#e2e8f0' };

export function stateColor(task: ApiPrdTask) {
  const state = (task.metadata?.state as string)?.toLowerCase() || 'todo';
  return STATE_COLORS[state] || DEFAULT_COLOR;
}
