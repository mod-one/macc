import { render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter } from 'react-router-dom';
import Registry from './Registry';

const getRegistryTasksMock = vi.fn();
const getConfigMock = vi.fn();

vi.mock('../../api/client', () => ({
  getRegistryTasks: (...args: unknown[]) => getRegistryTasksMock(...args),
  getConfig: (...args: unknown[]) => getConfigMock(...args),
  requeueTask: vi.fn(),
  reassignTask: vi.fn(),
  abandonTask: vi.fn(),
}));

describe('Registry page', () => {
  beforeEach(() => {
    getRegistryTasksMock.mockReset();
    getConfigMock.mockReset();
  });

  it('opens the selected task from navigation state', async () => {
    getRegistryTasksMock.mockResolvedValue([
      {
        id: 'task-1',
        title: 'First task',
        priority: '1',
        state: 'todo',
        tool: 'codex',
        attempts: 0,
        heartbeat: null,
        delayedUntil: null,
        currentPhase: 'plan',
        lastError: null,
        lastErrorCode: null,
        description: null,
        objective: null,
        result: null,
        steps: [],
        notes: null,
        assignee: null,
        worktree: null,
        events: [],
        updatedAt: null,
      },
      {
        id: 'task-2',
        title: 'Second task',
        priority: '2',
        state: 'active',
        tool: 'codex',
        attempts: 1,
        heartbeat: null,
        delayedUntil: null,
        currentPhase: 'dev',
        lastError: null,
        lastErrorCode: null,
        description: null,
        objective: null,
        result: null,
        steps: [],
        notes: null,
        assignee: null,
        worktree: null,
        events: [],
        updatedAt: null,
      },
    ]);
    getConfigMock.mockResolvedValue({ enabledTools: ['codex'] });

    render(
      <MemoryRouter
        initialEntries={[
          {
            pathname: '/ops/registry',
            state: { selectedTaskId: 'task-2' },
          },
        ]}
      >
        <Registry />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText('Current Phase')).toBeInTheDocument();
      expect(screen.getByText('dev')).toBeInTheDocument();
    });
  });
});
