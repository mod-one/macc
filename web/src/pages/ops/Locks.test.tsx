import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ApiRegistryTask, ApiWorktree } from '../../api/models';

const { fitViewMock, runActionMock } = vi.hoisted(() => ({
  fitViewMock: vi.fn(),
  runActionMock: vi.fn().mockResolvedValue({}),
}));

interface MockFlowNode {
  id: string;
  data?: {
    title?: ReactNode;
  };
}

interface ReactFlowMockProps {
  children?: ReactNode;
  nodes?: MockFlowNode[];
  onNodeClick?: (event: unknown, node: MockFlowNode) => void;
}

vi.mock('@xyflow/react', async () => {
  const actual = await vi.importActual<typeof import('@xyflow/react')>('@xyflow/react');

  function ReactFlowMock({ nodes, onNodeClick, children }: ReactFlowMockProps) {
    return (
      <div data-testid="react-flow">
        {children}
        <div>
          {nodes?.map((node) => (
            <button
              key={node.id}
              data-testid={`node-${node.id}`}
              onClick={(event) => onNodeClick?.(event, node)}
              type="button"
            >
              {node.data?.title ?? node.id}
            </button>
          ))}
        </div>
      </div>
    );
  }

  return {
    ...actual,
    Background: () => null,
    Controls: () => null,
    Handle: () => null,
    MarkerType: { ArrowClosed: 'arrowclosed' },
    MiniMap: () => null,
    Position: { Left: 'left', Right: 'right', Top: 'top', Bottom: 'bottom' },
    ReactFlow: ReactFlowMock,
    useReactFlow: () => ({ fitView: fitViewMock }),
  };
});

vi.mock('../../api/client', async () => {
  const actual = await vi.importActual<typeof import('../../api/client')>('../../api/client');
  return {
    ...actual,
    getRegistryTasks: vi.fn(),
    getWorktrees: vi.fn(),
  };
});

interface CoordinatorStoreState {
  status: null;
  loadError: null;
  isLoadingStatus: boolean;
  pendingAction: null;
  loadStatus: () => Promise<void>;
  runAction: typeof runActionMock;
}

vi.mock('../../store', () => ({
  useCoordinatorStore: <T,>(selector: (state: CoordinatorStoreState) => T) =>
    selector({
      status: null,
      loadError: null,
      isLoadingStatus: false,
      pendingAction: null,
      loadStatus: vi.fn().mockResolvedValue(undefined),
      runAction: runActionMock,
  }),
}));

const { default: Locks } = await import('./Locks');
const { getRegistryTasks, getWorktrees } = await import('../../api/client');

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

describe('Locks page', () => {
  beforeEach(() => {
    fitViewMock.mockClear();
    runActionMock.mockClear();
  });

  it('renders the graph, opens a details drawer, and runs coordinator shortcuts', async () => {
    vi.mocked(getRegistryTasks).mockResolvedValue([
      task('WEB-A', 'active', [], ['shared-cache'], '/tmp/worktrees/a', 'session-1'),
      task('WEB-B', 'blocked', ['WEB-A'], ['shared-cache']),
      task('WEB-C', 'todo', ['WEB-A'], ['shared-cache']),
    ]);
    vi.mocked(getWorktrees).mockResolvedValue([worktree('wt-a', '/tmp/worktrees/a', 'session-1')]);

    render(<Locks />);

    expect(await screen.findByText('Deadlock warning')).toBeInTheDocument();
    expect(screen.getByText('Dependency Graph & Locks')).toBeInTheDocument();
    expect(screen.getByText('Deadlock cycles')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /reset view/i }));
    expect(fitViewMock).toHaveBeenCalled();

    fireEvent.click(await screen.findByTestId('node-WEB-A'));

    expect(await screen.findByRole('heading', { name: 'WEB-A title' })).toBeInTheDocument();
    expect(screen.getByText('WEB-A description')).toBeInTheDocument();
    expect(screen.getByText('step one')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /reconcile/i }));
    fireEvent.click(screen.getByRole('button', { name: /unlock/i }));
    fireEvent.click(screen.getByRole('button', { name: /cleanup/i }));

    await waitFor(() => {
      expect(runActionMock).toHaveBeenCalledWith('reconcile');
      expect(runActionMock).toHaveBeenCalledWith('unlock');
      expect(runActionMock).toHaveBeenCalledWith('cleanup');
    });
  });
});
