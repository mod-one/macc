import { describe, expect, it } from 'vitest';
import type { ApiRegistryTask, ApiWorktree } from '../../api/models';
import { buildLocksGraph } from './locksGraph';

function task(
  id: string,
  state: string,
  dependencies: string[] = [],
  exclusiveResources: string[] = [],
  worktreePath: string | null = null,
  sessionId: string | null = null,
): ApiRegistryTask {
  return {
    id,
    title: `${id} title`,
    priority: 'P1',
    state,
    tool: 'codex',
    attempts: 0,
    heartbeat: null,
    delayedUntil: null,
    currentPhase: null,
    lastError: null,
    lastErrorCode: null,
    description: `${id} description`,
    objective: `${id} objective`,
    result: null,
    steps: ['step one', 'step two'],
    notes: null,
    assignee: null,
    worktree: worktreePath
      ? {
          worktreePath,
          branch: `${id.toLowerCase()}-branch`,
          baseBranch: 'main',
          lastCommit: null,
          sessionId,
        }
      : null,
    events: [],
    updatedAt: '2026-03-26T00:00:00Z',
    dependencies,
    exclusiveResources,
  };
}

function worktree(id: string, path: string, sessionLabel: string | null): ApiWorktree {
  return {
    id,
    slug: id,
    branch: `${id}-branch`,
    tool: 'codex',
    status: 'active',
    path,
    baseBranch: 'main',
    head: 'abc123',
    scope: 'user',
    feature: 'feature',
    locked: true,
    prunable: false,
    sessionLabel,
  };
}

describe('buildLocksGraph', () => {
  it('builds nodes, leases, and a deadlock cycle across tasks and resources', () => {
    const tasks = [
      task('WEB-A', 'active', [], ['shared-cache'], '/tmp/worktrees/a', 'session-1'),
      task('WEB-B', 'blocked', ['WEB-A'], ['shared-cache']),
      task('WEB-C', 'todo', ['WEB-A'], ['shared-cache']),
    ];
    const worktrees = [worktree('wt-a', '/tmp/worktrees/a', 'session-1')];

    const graph = buildLocksGraph(tasks, worktrees);

    expect(graph.nodes.map((node) => node.id)).toEqual(
      expect.arrayContaining(['WEB-A', 'WEB-B', 'WEB-C', 'resource:shared-cache', 'worktree:wt-a', 'session:session-1']),
    );
    expect(graph.edges.map((edge) => edge.id)).toEqual(
      expect.arrayContaining(['WEB-A->WEB-B', 'WEB-A->resource:shared-cache', 'WEB-B->resource:shared-cache']),
    );
    expect(graph.resourceOwners.get('shared-cache')).toBe('WEB-A');
    expect(graph.resourceContenders.get('shared-cache')).toEqual(['WEB-A', 'WEB-B', 'WEB-C']);
    expect(graph.worktreeByNodeId.get('worktree:wt-a')?.path).toBe('/tmp/worktrees/a');
    expect(graph.sessionByNodeId.get('session:session-1')?.tasks).toEqual(['WEB-A']);
    expect(graph.sessionByNodeId.get('session:session-1')?.worktrees).toEqual(['wt-a']);
    expect(graph.cycleNodeIds.has('WEB-A')).toBe(true);
    expect(graph.cycleNodeIds.has('WEB-B')).toBe(true);
    expect(graph.cycleNodeIds.has('resource:shared-cache')).toBe(true);
    expect(graph.cycleEdgeIds.size).toBeGreaterThan(0);
  });

  it('keeps acyclic graphs clean when there is no shared contention', () => {
    const graph = buildLocksGraph(
      [task('WEB-Z', 'todo', ['WEB-Y'], ['unique-resource'])],
      [],
    );

    expect(graph.cycleNodeIds.size).toBe(0);
    expect(graph.cycleEdgeIds.size).toBe(0);
    expect(graph.resourceOwners.get('unique-resource')).toBe('WEB-Z');
  });
});
