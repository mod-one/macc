import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import userEvent from '@testing-library/user-event';
import GlobalSearch from './GlobalSearch';
import { useGlobalSearchStore } from '../stores/globalSearchStore';

const getRegistryTasksMock = vi.fn();
const getWorktreesMock = vi.fn();
const getConfigMock = vi.fn();
const getLogsMock = vi.fn();

vi.mock('../api/client', () => ({
  getRegistryTasks: (...args: unknown[]) => getRegistryTasksMock(...args),
  getWorktrees: (...args: unknown[]) => getWorktreesMock(...args),
  getConfig: (...args: unknown[]) => getConfigMock(...args),
  getLogs: (...args: unknown[]) => getLogsMock(...args),
}));

function RouteProbe() {
  const location = useLocation();
  return <div data-testid="route-state">{JSON.stringify(location.state)}</div>;
}

describe('GlobalSearch', () => {
  beforeEach(() => {
    useGlobalSearchStore.setState({
      tasks: [],
      worktrees: [],
      config: null,
      logs: [],
      isLoading: false,
      error: null,
      lastLoadedAt: null,
    });
    getRegistryTasksMock.mockResolvedValue([]);
    getWorktreesMock.mockResolvedValue([]);
    getConfigMock.mockResolvedValue({
      enabledTools: [],
      toolConfig: {},
      toolSettings: {},
      standardsPath: null,
      standardsInline: {},
      selectedSkills: [],
      selectedAgents: [],
      selectedMcp: [],
      quiet: false,
      offline: false,
      webPort: 3450,
      webAssets: null,
      ralphEnabled: null,
      ralphIterationsDefault: null,
      ralphBranchName: null,
      ralphStopOnFailure: null,
      coordinatorTool: null,
      referenceBranch: null,
      prdFile: null,
      taskRegistryFile: null,
      toolPriority: [],
      maxParallelPerTool: {},
      toolSpecializations: {},
      maxDispatch: null,
      maxParallel: null,
      timeoutSeconds: null,
      phaseRunnerMaxAttempts: null,
      logFlushLines: null,
      logFlushMs: null,
      mirrorJsonDebounceMs: null,
      staleClaimedSeconds: null,
      staleInProgressSeconds: null,
      staleChangesRequestedSeconds: null,
      staleAction: null,
      storageMode: null,
      mergeAiFix: null,
      mergeJobTimeoutSeconds: null,
      mergeHookTimeoutSeconds: null,
      ghostHeartbeatGraceSeconds: null,
      dispatchCooldownSeconds: null,
      jsonCompat: null,
      legacyJsonFallback: null,
      errorCodeRetryList: null,
      errorCodeRetryMax: null,
      cutoverGateWindowEvents: null,
      cutoverGateMaxBlockedRatio: null,
      cutoverGateMaxStaleRatio: null,
      rateLimitBackoffBaseSeconds: null,
      rateLimitBackoffMaxSeconds: null,
      rateLimitFallbackEnabled: null,
      rateLimitThrottleParallel: null,
      requirementsDetected: false,
      managedEnvironmentWarnings: [],
    });
    getLogsMock.mockResolvedValue([]);
  });

  it('focuses with Ctrl+/ and navigates with keyboard selection', async () => {
    const user = userEvent.setup();
    getWorktreesMock.mockResolvedValue([
      {
        id: 'wt-a',
        slug: 'alpha worktree',
        branch: 'feat/alpha',
        tool: 'codex',
        status: 'active',
        path: '/tmp/alpha',
        baseBranch: 'main',
        head: 'head-a',
        scope: 'project',
        feature: 'alpha',
        locked: false,
        prunable: false,
        sessionLabel: null,
      },
    ]);

    render(
      <MemoryRouter initialEntries={['/']}>
        <Routes>
          <Route path="/" element={<GlobalSearch />} />
          <Route path="/ops/worktrees" element={<RouteProbe />} />
        </Routes>
      </MemoryRouter>,
    );

    fireEvent.keyDown(window, { key: '/', ctrlKey: true });
    expect(screen.getByRole('combobox')).toHaveFocus();

    const input = screen.getByRole('combobox');
    fireEvent.change(input, { target: { value: 'worktree' } });

    await new Promise((resolve) => setTimeout(resolve, 350));

    expect(screen.getByRole('button', { name: /alpha worktree/i })).toBeInTheDocument();

    await act(async () => {
      await user.keyboard('{ArrowDown}');
    });
    await act(async () => {
      await user.keyboard('{Enter}');
    });

    expect(screen.getByTestId('route-state')).toHaveTextContent(
      '"selectedWorktreeId":"wt-a"',
    );
  });
});
