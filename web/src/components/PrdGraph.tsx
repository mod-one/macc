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
import {
  detectCycles,
  findCriticalPath,
  layoutNodes,
  stateColor,
  NODE_W,
  NODE_H,
} from './prdGraphAlgorithms';

// ---------- Typed node data ----------

interface PrdNodeData {
  label: React.ReactNode;
  title: string | null;
  priority: string | null;
  [key: string]: unknown;
}

type PrdNode = Node<PrdNodeData>;

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

    const ns: PrdNode[] = tasks.map(t => {
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

  const onNodeClick: NodeMouseHandler<PrdNode> = useCallback((_event, node) => {
    onSelectTask(node.id);
  }, [onSelectTask]);

  const nodeLabel = useCallback((node: PrdNode) => {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 2 }}>
        <span style={{ fontWeight: 600, fontSize: 11 }}>{String(node.data.label)}</span>
        {node.data.title && (
          <span style={{ fontSize: 10, opacity: 0.7, maxWidth: NODE_W - 20, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {node.data.title}
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
