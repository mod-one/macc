import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  ApiConfigResponse,
  ApiStandardsPreviewRequest,
  ApiStandardsPreviewResponse,
} from '../../api/models';
import Standards from './Standards';

const getConfigMock = vi.fn();
const getStandardsPreviewMock = vi.fn();
const updateConfigMock = vi.fn();

vi.mock('../../api/client', () => ({
  getConfig: (...args: unknown[]) => getConfigMock(...args),
  getStandardsPreview: (...args: unknown[]) => getStandardsPreviewMock(...args),
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

function buildConfig(overrides: Partial<ApiConfigResponse> = {}): ApiConfigResponse {
  return {
    version: null,
    enabledTools: ['codex', 'claude', 'gemini'],
    toolConfig: {},
    toolSettings: {},
    standardsPath: null,
    standardsInline: {
      language: 'English',
      package_manager: 'pnpm',
    },
    selectedSkills: [],
    mandatorySkills: [],
    selectedAgents: [],
    selectedMcp: [],
    quiet: false,
    debug: false,
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
    toolPriority: ['codex'],
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
    forceKillGraceSeconds: null,
    requirementsDetected: false,
    managedEnvironmentWarnings: [],
    ...overrides,
  };
}

function buildStandardsPreview(
  overrides: Partial<ApiStandardsPreviewResponse> = {},
): ApiStandardsPreviewResponse {
  return {
    cards: [
      {
        id: 'codex',
        title: 'Codex - AGENTS.md (rendered)',
        content: '# Project Instructions (MACC)\n\n## Standards\n- language: English\n',
      },
      {
        id: 'claude',
        title: 'Claude - CLAUDE.md (rendered)',
        content: '# Project Instructions (MACC)\n\n- **Primary Model**: opus\n- **Language**: English\n',
      },
      {
        id: 'gemini',
        title: 'Gemini - GEMINI.md (rendered)',
        content: '# Project Instructions (MACC)\n\n## Standards (summary)\n- language: English\n',
      },
    ],
    ...overrides,
  };
}

describe('Standards page', () => {
  beforeEach(() => {
    getConfigMock.mockReset();
    getStandardsPreviewMock.mockReset();
    updateConfigMock.mockReset();
    getStandardsPreviewMock.mockResolvedValue(buildStandardsPreview());
  });

  it('renders editor, diff, lint, and API-backed preview sections', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    render(<Standards />);

    await screen.findByText('Standards');
    await waitFor(() => {
      expect(getStandardsPreviewMock).toHaveBeenCalledTimes(1);
    });

    const previewRequest = getStandardsPreviewMock.mock.calls[0][0] as ApiStandardsPreviewRequest;
    expect(previewRequest).toMatchObject({
      standardsPath: null,
      standardsInline: {
        language: 'English',
        package_manager: 'pnpm',
      },
    });

    expect(screen.getByLabelText('Standards preset')).toBeInTheDocument();
    expect(screen.getByText('Override editor')).toBeInTheDocument();
    expect(screen.getByText('Diff from preset')).toBeInTheDocument();
    expect(screen.getByText('Lint warnings')).toBeInTheDocument();
    expect(screen.getByText('Rendered output preview')).toBeInTheDocument();
    expect(screen.getByText('Codex - AGENTS.md (rendered)')).toBeInTheDocument();
    expect(screen.getByText(/Primary Model/)).toBeInTheDocument();
  });

  it('supports preset changes and override edits', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    render(<Standards />);

    const preset = await screen.findByLabelText('Standards preset');
    fireEvent.change(preset, { target: { value: 'strict' } });

    expect(screen.getAllByText('strict').length).toBeGreaterThan(0);
    expect(screen.getByText('imports')).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText('convention key'), {
      target: { value: 'commit_format' },
    });
    fireEvent.change(screen.getByPlaceholderText('value'), {
      target: { value: 'conventional' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add override' }));

    expect(screen.getAllByText('commit_format').length).toBeGreaterThan(0);
    expect(screen.getAllByText('added').length).toBeGreaterThan(0);
  });

  it('shows lint warning for inconsistent language', async () => {
    getConfigMock.mockResolvedValue(
      buildConfig({
        standardsInline: {
          language: 'Spanish',
        },
      }),
    );
    render(<Standards />);

    expect(
      await screen.findByText('language should be "English" for consistency (current: Spanish).'),
    ).toBeInTheDocument();
  });

  it('saves standards section through updateConfig', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    updateConfigMock.mockResolvedValue(
      buildConfig({
        standardsInline: {
          language: 'English',
          package_manager: 'pnpm',
          commit_format: 'conventional',
        },
      }),
    );

    render(<Standards />);
    await screen.findByText('Standards');

    fireEvent.change(screen.getByPlaceholderText('convention key'), {
      target: { value: 'commit_format' },
    });
    fireEvent.change(screen.getByPlaceholderText('value'), {
      target: { value: 'conventional' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add override' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save standards' }));

    await waitFor(() => {
      expect(updateConfigMock).toHaveBeenCalledTimes(1);
    });
    expect(updateConfigMock.mock.calls[0][0]).toMatchObject({
      standardsPath: null,
      standardsInline: {
        language: 'English',
        package_manager: 'pnpm',
        commit_format: 'conventional',
      },
    });
    expect(await screen.findByText('Standards saved')).toBeInTheDocument();
  });
});
