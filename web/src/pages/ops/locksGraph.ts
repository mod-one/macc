import { MarkerType, Position, type Edge, type Node } from '@xyflow/react';
import type { ApiRegistryTask, ApiWorktree } from '../../api/models';

export type LocksNodeKind = 'task' | 'resource' | 'worktree' | 'session';

export interface LocksNodeData extends Record<string, unknown> {
  kind: LocksNodeKind;
  title: string;
  subtitle: string | null;
  summary: string[];
  accent: string;
  badge: string | null;
  status: string | null;
  count: number | null;
}

export type LocksNode = Node<LocksNodeData>;

export interface LocksGraphResult {
  nodes: LocksNode[];
  edges: Edge[];
  cycleNodeIds: Set<string>;
  cycleEdgeIds: Set<string>;
  resourceOwners: Map<string, string | null>;
  resourceContenders: Map<string, string[]>;
  taskById: Map<string, ApiRegistryTask>;
  worktreeByNodeId: Map<string, ApiWorktree>;
  sessionByNodeId: Map<string, { id: string; label: string; tasks: string[]; worktrees: string[] }>;
}

interface GraphNodeRecord {
  id: string;
  kind: LocksNodeKind;
  title: string;
  subtitle: string | null;
  summary: string[];
  accent: string;
  badge: string | null;
  status: string | null;
  count: number | null;
  position: { x: number; y: number };
  sourcePosition: Position;
  targetPosition: Position;
}

const NODE_WIDTH = 250;
const NODE_HEIGHT = 88;
const COLUMN_GAP = 320;
const ROW_GAP = 140;

const TASK_ACCENTS: Record<string, string> = {
  todo: '#64748b',
  queued: '#64748b',
  active: '#38bdf8',
  in_progress: '#38bdf8',
  blocked: '#f97316',
  failed: '#ef4444',
  merged: '#22c55e',
  done: '#22c55e',
};

const RESOURCE_ACCENT = '#f59e0b';
const WORKTREE_ACCENT = '#c084fc';
const SESSION_ACCENT = '#14b8a6';

function normalizeState(state: string | null): string {
  return (state ?? 'todo').toLowerCase();
}

function statusTone(state: string | null): string {
  return TASK_ACCENTS[normalizeState(state)] ?? TASK_ACCENTS.todo;
}

function safeList(value: string[] | null | undefined): string[] {
  return (value ?? []).filter((entry) => entry.trim().length > 0);
}

function taskDepth(taskId: string, dependencyMap: Map<string, string[]>, memo: Map<string, number>, visiting: Set<string>): number {
  const cached = memo.get(taskId);
  if (cached !== undefined) {
    return cached;
  }

  if (visiting.has(taskId)) {
    return 0;
  }

  visiting.add(taskId);
  const parents = dependencyMap.get(taskId) ?? [];
  const depth = parents.length === 0
    ? 0
    : Math.max(...parents.map((parent) => taskDepth(parent, dependencyMap, memo, visiting) + 1));
  visiting.delete(taskId);
  memo.set(taskId, depth);
  return depth;
}

function detectCycles(edges: Array<{ source: string; target: string }>): {
  cycleNodeIds: Set<string>;
  cycleEdgeIds: Set<string>;
} {
  const adjacency = new Map<string, string[]>();
  const ids = new Set<string>();
  for (const edge of edges) {
    ids.add(edge.source);
    ids.add(edge.target);
    const list = adjacency.get(edge.source) ?? [];
    list.push(edge.target);
    adjacency.set(edge.source, list);
  }

  const WHITE = 0;
  const GRAY = 1;
  const BLACK = 2;
  const color = new Map<string, number>();
  const cycleNodeIds = new Set<string>();
  const cycleEdgeIds = new Set<string>();
  const stack: string[] = [];

  for (const id of ids) {
    color.set(id, WHITE);
  }

  function dfs(nodeId: string) {
    color.set(nodeId, GRAY);
    stack.push(nodeId);

    for (const next of adjacency.get(nodeId) ?? []) {
      const edgeId = `${nodeId}->${next}`;
      if (color.get(next) === GRAY) {
        cycleEdgeIds.add(edgeId);
        cycleNodeIds.add(next);
        for (let index = stack.length - 1; index >= 0; index -= 1) {
          const item = stack[index];
          cycleNodeIds.add(item);
          if (item === next) {
            break;
          }
        }
        continue;
      }

      if (color.get(next) === WHITE) {
        dfs(next);
      }
    }

    stack.pop();
    color.set(nodeId, BLACK);
  }

  for (const id of ids) {
    if (color.get(id) === WHITE) {
      dfs(id);
    }
  }

  return { cycleNodeIds, cycleEdgeIds };
}

function sortByCountDescending(entries: Array<[string, number]>): Array<[string, number]> {
  return [...entries].sort((left, right) => {
    if (right[1] !== left[1]) {
      return right[1] - left[1];
    }
    return left[0].localeCompare(right[0]);
  });
}

function chooseResourceOwner(contenders: ApiRegistryTask[]): ApiRegistryTask | null {
  if (contenders.length === 0) {
    return null;
  }

  const ranked = [...contenders].sort((left, right) => {
    const leftActive = ['active', 'in_progress'].includes(normalizeState(left.state));
    const rightActive = ['active', 'in_progress'].includes(normalizeState(right.state));
    if (leftActive !== rightActive) {
      return leftActive ? -1 : 1;
    }

    const leftLocked = Boolean(left.worktree?.worktreePath) && Boolean(left.worktree?.sessionId);
    const rightLocked = Boolean(right.worktree?.worktreePath) && Boolean(right.worktree?.sessionId);
    if (leftLocked !== rightLocked) {
      return leftLocked ? -1 : 1;
    }

    return left.id.localeCompare(right.id);
  });

  return ranked[0] ?? null;
}

function buildResourceSummary(contenders: ApiRegistryTask[], ownerId: string | null): string[] {
  const summary = [`Shared by ${contenders.length} task${contenders.length === 1 ? '' : 's'}`];
  if (ownerId) {
    summary.push(`Owner: ${ownerId}`);
  }
  const blockedCount = contenders.filter((task) => normalizeState(task.state) === 'blocked').length;
  if (blockedCount > 0) {
    summary.push(`${blockedCount} blocked contender${blockedCount === 1 ? '' : 's'}`);
  }
  return summary;
}

function buildNodeRecord(record: GraphNodeRecord): LocksNode {
  return {
    id: record.id,
    type: record.kind,
    position: record.position,
    sourcePosition: record.sourcePosition,
    targetPosition: record.targetPosition,
    data: {
      kind: record.kind,
      title: record.title,
      subtitle: record.subtitle,
      summary: record.summary,
      accent: record.accent,
      badge: record.badge,
      status: record.status,
      count: record.count,
    },
    style: {
      width: NODE_WIDTH,
      minHeight: NODE_HEIGHT,
    },
  };
}

export function buildLocksGraph(tasks: ApiRegistryTask[], worktrees: ApiWorktree[]): LocksGraphResult {
  const taskById = new Map(tasks.map((task) => [task.id, task]));
  const worktreeByNodeId = new Map<string, ApiWorktree>();
  const dependencyMap = new Map<string, string[]>();
  const logicalEdges: Array<{ source: string; target: string }> = [];
  const resourceContenders = new Map<string, string[]>();
  const resourceOwners = new Map<string, string | null>();
  const taskDepthMemo = new Map<string, number>();
  const taskNodes: GraphNodeRecord[] = [];
  const resourceNodes: GraphNodeRecord[] = [];
  const worktreeNodes: GraphNodeRecord[] = [];
  const sessionNodes: GraphNodeRecord[] = [];

  for (const task of tasks) {
    dependencyMap.set(task.id, safeList(task.dependencies).filter((dependency) => taskById.has(dependency)));
  }

  const taskIds = tasks.map((task) => task.id);
  const taskHeights = new Map<string, number>();
  for (const taskId of taskIds) {
    taskHeights.set(taskId, taskDepth(taskId, dependencyMap, taskDepthMemo, new Set()));
  }

  const taskLayers = new Map<number, string[]>();
  for (const task of tasks) {
    const depth = taskHeights.get(task.id) ?? 0;
    const layer = taskLayers.get(depth) ?? [];
    layer.push(task.id);
    taskLayers.set(depth, layer);
  }

  const maxTaskLayer = taskLayers.size > 0 ? Math.max(...taskLayers.keys()) : 0;

  for (const [layer, ids] of taskLayers) {
    const sorted = [...ids].sort((left, right) => left.localeCompare(right));
    const startY = -((sorted.length - 1) * ROW_GAP) / 2;
    sorted.forEach((taskId, index) => {
      const task = taskById.get(taskId);
      if (!task) {
        return;
      }
      const state = normalizeState(task.state);
      const accent = statusTone(task.state);
      const summary = [
        task.objective ?? task.description ?? 'No description provided',
        ...safeList(task.steps).slice(0, 2).map((step) => `• ${step}`),
      ].filter((entry): entry is string => entry.trim().length > 0);
      taskNodes.push({
        id: task.id,
        kind: 'task',
        title: task.title ?? task.id,
        subtitle: `${state}${task.priority ? ` • ${task.priority}` : ''}`,
        summary,
        accent,
        badge: task.tool ?? null,
        status: task.state,
        count: task.dependencies.length,
        position: {
          x: layer * COLUMN_GAP,
          y: startY + index * ROW_GAP,
        },
        sourcePosition: Position.Right,
        targetPosition: Position.Left,
      });

      for (const dependency of dependencyMap.get(task.id) ?? []) {
        logicalEdges.push({ source: dependency, target: task.id });
      }
    });
  }

  const resourceMap = new Map<string, Set<string>>();
  for (const task of tasks) {
    for (const resource of safeList(task.exclusiveResources)) {
      const contenders = resourceMap.get(resource) ?? new Set<string>();
      contenders.add(task.id);
      resourceMap.set(resource, contenders);
    }
  }

  const resourceEntries = sortByCountDescending(
    Array.from(resourceMap.entries()).map(([resource, contenders]): [string, number] => [resource, contenders.size]),
  );

  resourceEntries.forEach(([resource, count], index) => {
    const contenders = Array.from(resourceMap.get(resource) ?? [])
      .map((taskId) => taskById.get(taskId))
      .filter((task): task is ApiRegistryTask => task !== undefined);
    const owner = chooseResourceOwner(contenders);
    const ownerId = owner?.id ?? null;
    resourceOwners.set(resource, ownerId);
    resourceContenders.set(resource, contenders.map((task) => task.id));

    const summary = buildResourceSummary(contenders, ownerId);
    resourceNodes.push({
      id: `resource:${resource}`,
      kind: 'resource',
      title: resource,
      subtitle: 'Exclusive resource',
      summary,
      accent: RESOURCE_ACCENT,
      badge: `${count} claim${count === 1 ? '' : 's'}`,
      status: null,
      count,
      position: {
        x: (maxTaskLayer + 1) * COLUMN_GAP,
        y: -((resourceEntries.length - 1) * ROW_GAP) / 2 + index * ROW_GAP,
      },
      sourcePosition: Position.Right,
      targetPosition: Position.Left,
    });

    for (const contender of contenders) {
      logicalEdges.push({ source: contender.id, target: `resource:${resource}` });
    }

    if (ownerId && contenders.some((task) => task.id !== ownerId)) {
      logicalEdges.push({ source: `resource:${resource}`, target: ownerId });
    }
  });

  const sessionMap = new Map<string, { tasks: Set<string>; worktrees: Set<string> }>();
  for (const task of tasks) {
    const sessionId = task.worktree?.sessionId?.trim();
    if (sessionId) {
      const entry = sessionMap.get(sessionId) ?? { tasks: new Set<string>(), worktrees: new Set<string>() };
      entry.tasks.add(task.id);
      sessionMap.set(sessionId, entry);
    }
  }

  for (const worktree of worktrees) {
    const sessionId = worktree.sessionLabel?.trim();
    if (!sessionId) {
      continue;
    }
    const entry = sessionMap.get(sessionId) ?? { tasks: new Set<string>(), worktrees: new Set<string>() };
    entry.worktrees.add(worktree.id);
    sessionMap.set(sessionId, entry);
  }

  const sessionEntries = sortByCountDescending(
    Array.from(sessionMap.entries()).map(([sessionId, entry]): [string, number] => [
      sessionId,
      entry.tasks.size + entry.worktrees.size,
    ]),
  );

  sessionEntries.forEach(([sessionId, count], index) => {
    const entry = sessionMap.get(sessionId);
    const tasksForSession = Array.from(entry?.tasks ?? []).sort((left, right) => left.localeCompare(right));
    const worktreesForSession = Array.from(entry?.worktrees ?? []).sort((left, right) => left.localeCompare(right));

    sessionNodes.push({
      id: `session:${sessionId}`,
      kind: 'session',
      title: sessionId,
      subtitle: 'Tool session lease',
      summary: [
        `${tasksForSession.length} task${tasksForSession.length === 1 ? '' : 's'}`,
        `${worktreesForSession.length} worktree${worktreesForSession.length === 1 ? '' : 's'}`,
      ],
      accent: SESSION_ACCENT,
      badge: 'Lease',
      status: null,
      count,
      position: {
        x: (maxTaskLayer + 2) * COLUMN_GAP,
        y: -((sessionEntries.length - 1) * ROW_GAP) / 2 + index * ROW_GAP,
      },
      sourcePosition: Position.Right,
      targetPosition: Position.Left,
    });
  });

  worktrees.forEach((worktree, index) => {
    const nodeId = `worktree:${worktree.id}`;
    worktreeByNodeId.set(nodeId, worktree);
    const sessionId = worktree.sessionLabel?.trim();
    const linkedTasks = tasks.filter((task) => {
      const taskPath = task.worktree?.worktreePath?.trim();
      return taskPath !== null && taskPath === worktree.path.trim();
    });

    worktreeNodes.push({
      id: nodeId,
      kind: 'worktree',
      title: worktree.slug ?? worktree.id,
      subtitle: worktree.branch ?? worktree.path,
      summary: [
        worktree.path,
        worktree.locked ? 'Locked slot' : 'Open slot',
        worktree.prunable ? 'Prunable' : 'Retained',
      ],
      accent: WORKTREE_ACCENT,
      badge: worktree.tool ?? null,
      status: worktree.status,
      count: linkedTasks.length,
      position: {
        x: (maxTaskLayer + 2) * COLUMN_GAP,
        y: -((worktrees.length - 1) * ROW_GAP) / 2 + index * ROW_GAP,
      },
      sourcePosition: Position.Right,
      targetPosition: Position.Left,
    });

    if (sessionId) {
      logicalEdges.push({ source: nodeId, target: `session:${sessionId}` });
    }

    for (const task of linkedTasks) {
      logicalEdges.push({ source: task.id, target: nodeId });
    }
  });

  const cycle = detectCycles(logicalEdges);
  const nodes = [...taskNodes, ...resourceNodes, ...worktreeNodes, ...sessionNodes].map((node) => buildNodeRecord(node));

  const edges: Edge[] = [];
  for (const edge of logicalEdges) {
    const isDependency = taskById.has(edge.source) && taskById.has(edge.target);
    const isContention = edge.source.startsWith('resource:') || edge.target.startsWith('resource:');
    const isLease = edge.source.startsWith('session:') || edge.target.startsWith('session:');
    const isAllocation = edge.source.startsWith('worktree:') || edge.target.startsWith('worktree:');
    const edgeId = `${edge.source}->${edge.target}`;
    edges.push({
      id: edgeId,
      source: edge.source,
      target: edge.target,
      type: 'smoothstep',
      markerEnd: { type: MarkerType.ArrowClosed, width: 18, height: 18 },
      animated: cycle.cycleEdgeIds.has(edgeId),
      style: {
        stroke: cycle.cycleEdgeIds.has(edgeId)
          ? '#f43f5e'
          : isDependency
            ? '#60a5fa'
            : isContention
              ? '#f59e0b'
              : isLease
                ? '#14b8a6'
                : isAllocation
                  ? '#c084fc'
                  : '#94a3b8',
        strokeWidth: cycle.cycleEdgeIds.has(edgeId) ? 2.8 : isDependency || isContention ? 2.1 : 1.6,
      },
    });
  }

  const sessions = new Map<string, { id: string; label: string; tasks: string[]; worktrees: string[] }>();
  for (const [sessionId, entry] of sessionMap.entries()) {
    sessions.set(`session:${sessionId}`, {
      id: sessionId,
      label: sessionId,
      tasks: Array.from(entry.tasks).sort((left, right) => left.localeCompare(right)),
      worktrees: Array.from(entry.worktrees).sort((left, right) => left.localeCompare(right)),
    });
  }

  return {
    nodes,
    edges,
    cycleNodeIds: cycle.cycleNodeIds,
    cycleEdgeIds: cycle.cycleEdgeIds,
    resourceOwners,
    resourceContenders,
    taskById,
    worktreeByNodeId,
    sessionByNodeId: sessions,
  };
}
