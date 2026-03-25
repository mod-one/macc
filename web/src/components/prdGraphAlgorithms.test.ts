import { describe, it, expect } from 'vitest';
import { detectCycles, findCriticalPath } from './prdGraphAlgorithms';
import type { ApiPrdTask } from '../api/models';

/** Helper to build a minimal ApiPrdTask with just id and dependencies. */
function task(id: string, dependencies: string[] = []): ApiPrdTask {
  return {
    id,
    title: null,
    priority: null,
    category: null,
    scope: null,
    baseBranch: null,
    coordinatorTool: null,
    description: null,
    objective: null,
    result: null,
    dependencies,
    exclusiveResources: [],
    steps: [],
    notes: null,
    metadata: {},
  };
}

// ── detectCycles ──

describe('detectCycles', () => {
  it('returns no cycles for empty input', () => {
    const result = detectCycles([]);
    expect(result.hasCycles).toBe(false);
    expect(result.cycleEdges.size).toBe(0);
  });

  it('returns no cycles for tasks without dependencies', () => {
    const result = detectCycles([task('A'), task('B'), task('C')]);
    expect(result.hasCycles).toBe(false);
  });

  it('returns no cycles for a linear chain', () => {
    const result = detectCycles([
      task('A'),
      task('B', ['A']),
      task('C', ['B']),
    ]);
    expect(result.hasCycles).toBe(false);
  });

  it('returns no cycles for a diamond DAG', () => {
    const result = detectCycles([
      task('A'),
      task('B', ['A']),
      task('C', ['A']),
      task('D', ['B', 'C']),
    ]);
    expect(result.hasCycles).toBe(false);
  });

  it('detects a self-loop', () => {
    const result = detectCycles([task('A', ['A'])]);
    expect(result.hasCycles).toBe(true);
    expect(result.cycleNodeIds.has('A')).toBe(true);
    expect(result.cycleEdges.has('A->A')).toBe(true);
  });

  it('detects a simple two-node cycle', () => {
    const result = detectCycles([
      task('A', ['B']),
      task('B', ['A']),
    ]);
    expect(result.hasCycles).toBe(true);
    expect(result.cycleNodeIds.has('A')).toBe(true);
    expect(result.cycleNodeIds.has('B')).toBe(true);
  });

  it('detects a three-node cycle', () => {
    const result = detectCycles([
      task('A', ['C']),
      task('B', ['A']),
      task('C', ['B']),
    ]);
    expect(result.hasCycles).toBe(true);
    expect(result.cycleNodeIds.size).toBeGreaterThanOrEqual(2);
  });

  it('ignores dependencies referencing non-existent tasks', () => {
    const result = detectCycles([
      task('A', ['Z']), // Z does not exist
      task('B', ['A']),
    ]);
    expect(result.hasCycles).toBe(false);
  });

  it('detects cycle in a graph with both cyclic and acyclic parts', () => {
    const result = detectCycles([
      task('A'),
      task('B', ['A']),
      task('C', ['B']),
      // D and E form a cycle
      task('D', ['E']),
      task('E', ['D']),
    ]);
    expect(result.hasCycles).toBe(true);
    expect(result.cycleNodeIds.has('D')).toBe(true);
    expect(result.cycleNodeIds.has('E')).toBe(true);
    // A, B, C are not in a cycle
    expect(result.cycleNodeIds.has('A')).toBe(false);
    expect(result.cycleNodeIds.has('B')).toBe(false);
  });
});

// ── findCriticalPath ──

describe('findCriticalPath', () => {
  it('returns empty set for empty input', () => {
    const result = findCriticalPath([]);
    expect(result.size).toBe(0);
  });

  it('returns single node for a single task', () => {
    const result = findCriticalPath([task('A')]);
    expect(result.size).toBe(1);
    expect(result.has('A')).toBe(true);
  });

  it('returns all nodes for a linear chain', () => {
    const result = findCriticalPath([
      task('A'),
      task('B', ['A']),
      task('C', ['B']),
    ]);
    expect(result.size).toBe(3);
    expect(result.has('A')).toBe(true);
    expect(result.has('B')).toBe(true);
    expect(result.has('C')).toBe(true);
  });

  it('finds the longest path in a diamond DAG', () => {
    // A -> B -> D (length 2)
    // A -> C -> D (length 2)
    // Both paths are equal, critical path should include A and D plus one middle node
    const result = findCriticalPath([
      task('A'),
      task('B', ['A']),
      task('C', ['A']),
      task('D', ['B', 'C']),
    ]);
    expect(result.size).toBe(3); // A, one of {B,C}, D
    expect(result.has('A')).toBe(true);
    expect(result.has('D')).toBe(true);
  });

  it('picks the longer branch', () => {
    // A -> B -> C -> D (length 3)
    // A -> E -> D       (length 2)
    const result = findCriticalPath([
      task('A'),
      task('B', ['A']),
      task('C', ['B']),
      task('D', ['C', 'E']),
      task('E', ['A']),
    ]);
    expect(result.size).toBe(4); // A, B, C, D
    expect(result.has('A')).toBe(true);
    expect(result.has('B')).toBe(true);
    expect(result.has('C')).toBe(true);
    expect(result.has('D')).toBe(true);
  });

  it('returns empty set when cycle prevents topological sort', () => {
    const result = findCriticalPath([
      task('A', ['B']),
      task('B', ['A']),
    ]);
    expect(result.size).toBe(0);
  });

  it('handles disconnected components', () => {
    // Component 1: A -> B -> C (length 2)
    // Component 2: X -> Y (length 1)
    const result = findCriticalPath([
      task('A'),
      task('B', ['A']),
      task('C', ['B']),
      task('X'),
      task('Y', ['X']),
    ]);
    // Critical path is the longest: A -> B -> C
    expect(result.size).toBe(3);
    expect(result.has('A')).toBe(true);
    expect(result.has('B')).toBe(true);
    expect(result.has('C')).toBe(true);
  });

  it('handles tasks with no dependencies (all roots)', () => {
    const result = findCriticalPath([
      task('A'),
      task('B'),
      task('C'),
    ]);
    // All have distance 0, critical path is just one node
    expect(result.size).toBe(1);
  });
});
