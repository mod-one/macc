import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ApiConfigResponse, ApiPlanResponse, ApiStandardsPreviewResponse } from '../api/models';
import Init from './Init';

const getConfigMock = vi.fn();
const getStandardsPreviewMock = vi.fn();
const runPlanMock = vi.fn();
const updateConfigMock = vi.fn();
const navigateMock = vi.fn();

vi.mock('../api/client', () => ({
  getConfig: (...args: unknown[]) => getConfigMock(...args),
  getStandardsPreview: (...args: unknown[]) => getStandardsPreviewMock(...args),
  runPlan: (...args: unknown[]) => runPlanMock(...args),
  updateConfig: (...args: unknown[]) => updateConfigMock(...args),
  ApiClientError: class ApiClientError extends Error {
    envelope = {
      error: {
        code: 'MACC-WEB-0000',
        message: 'Mock error',
      },
    };
  },
}));

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom');
  return {
    ...actual,
    useNavigate: () => navigateMock,
  };
});

function buildConfig(overrides: Partial<ApiConfigResponse> = {}): ApiConfigResponse {
  return {
    version: 'v1',
    enabledTools: ['codex', 'claude'],
    toolConfig: {
      codex: { version: '1.0.0', health: 'healthy' },
      claude: { version: '2.0.0', health: 'healthy' },
    },
    toolSettings: {
      codex: { version: '1.0.0', healthy: true },
      claude: { version: '2.0.0', healthy: true },
    },
    standardsPath: null,
    standardsInline: {
      language: 'English',
      package_manager: 'pnpm',
    },
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
    coordinatorTool: 'codex',
    referenceBranch: 'main',
    prdFile: 'prd.json',
    taskRegistryFile: null,
    toolPriority: ['codex', 'claude'],
    maxParallelPerTool: {},
    toolSpecializations: {},
    maxDispatch: 5,
    maxParallel: 3,
    timeoutSeconds: 1800,
    phaseRunnerMaxAttempts: 2,
    logFlushLines: null,
    logFlushMs: null,
    mirrorJsonDebounceMs: null,
    staleClaimedSeconds: 300,
    staleInProgressSeconds: 600,
    staleChangesRequestedSeconds: 900,
    staleAction: 'retry',
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
    requirementsDetected: true,
    managedEnvironmentWarnings: [],
    ...overrides,
  };
}

function buildStandardsPreview(): ApiStandardsPreviewResponse {
  return {
    cards: [
      { id: 'codex', title: 'Codex - AGENTS.md (rendered)', content: '# Project Instructions (MACC)\n- language: English' },
      { id: 'claude', title: 'Claude - CLAUDE.md (rendered)', content: '# Project Instructions (MACC)\n- language: English' },
    ],
  };
}

function buildPlanPreview(): ApiPlanResponse {
  return {
    summary: {
      totalActions: 2,
      filesWrite: 1,
      filesMerge: 0,
      consentRequired: 0,
      backupRequired: 0,
      backupPath: '/repo/.macc/backups',
    },
    files: [],
    diffs: [],
    risks: [{ level: 'safe', message: 'No risky writes detected.' }],
    consents: [],
  };
}

describe('Init page', () => {
  beforeEach(() => {
    getConfigMock.mockReset();
    getStandardsPreviewMock.mockReset();
    runPlanMock.mockReset();
    updateConfigMock.mockReset();
    navigateMock.mockReset();
    getConfigMock.mockResolvedValue(buildConfig());
    getStandardsPreviewMock.mockResolvedValue(buildStandardsPreview());
    runPlanMock.mockResolvedValue(buildPlanPreview());
    updateConfigMock.mockResolvedValue(buildConfig());
  });

  it('walks through the four steps with validation and plan preview', async () => {
    const user = userEvent.setup();
    render(<Init />);

    await screen.findByText('Project initialization wizard');
    expect(screen.getByText('Step 1')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Next' }));
    await user.click(screen.getByRole('button', { name: 'Next' }));

    expect(await screen.findByText('Standards preset')).toBeInTheDocument();
    expect(getStandardsPreviewMock).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole('button', { name: 'Next' }));
    expect(await screen.findByText('Config preview')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Preview plan' }));
    expect(await screen.findByText('Total actions: 2')).toBeInTheDocument();

    await user.click(screen.getByRole('checkbox', { name: 'I reviewed the config preview' }));
    await user.click(screen.getByRole('button', { name: 'Create project' }));

    await waitFor(() => {
      expect(updateConfigMock).toHaveBeenCalledTimes(1);
    });
    expect(updateConfigMock.mock.calls[0][0]).toMatchObject({
      enabledTools: ['claude', 'codex'],
      standardsPath: null,
      standardsInline: {
        language: 'English',
        package_manager: 'pnpm',
      },
    });
    expect(navigateMock).toHaveBeenCalledWith('/dashboard');
  });

  it('supports skipping to defaults before saving', async () => {
    const user = userEvent.setup();
    render(<Init />);

    await screen.findByText('Project initialization wizard');
    await user.click(screen.getByRole('button', { name: 'Skip to defaults' }));

    expect(await screen.findByText('Config preview')).toBeInTheDocument();
    await user.click(screen.getByRole('checkbox', { name: 'I reviewed the config preview' }));
    await user.click(screen.getByRole('button', { name: 'Create project' }));

    await waitFor(() => {
      expect(updateConfigMock).toHaveBeenCalledTimes(1);
    });
    expect(navigateMock).toHaveBeenCalledWith('/dashboard');
  });

  it('revalidates earlier steps when finishing from defaults', async () => {
    getConfigMock.mockResolvedValueOnce(
      buildConfig({
        enabledTools: [],
        toolConfig: {},
        toolSettings: {},
        toolPriority: [],
      }),
    );

    const user = userEvent.setup();
    render(<Init />);

    await screen.findByText('Project initialization wizard');
    await user.click(screen.getByRole('button', { name: 'Skip to defaults' }));
    expect(await screen.findByText('Config preview')).toBeInTheDocument();
    await user.click(screen.getByRole('checkbox', { name: 'I reviewed the config preview' }));
    await user.click(screen.getByRole('button', { name: 'Create project' }));

    expect(await screen.findByText('Enable at least one tool before continuing.')).toBeInTheDocument();
    expect(updateConfigMock).not.toHaveBeenCalled();
    expect(navigateMock).not.toHaveBeenCalled();
  });
});
