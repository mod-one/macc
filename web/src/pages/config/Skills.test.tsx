import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ApiConfigResponse } from '../../api/models';
import Skills from './Skills';

const getConfigMock = vi.fn();
const updateConfigMock = vi.fn();

vi.mock('../../api/client', () => ({
  getConfig: (...args: unknown[]) => getConfigMock(...args),
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
    enabledTools: [],
    toolConfig: {},
    toolSettings: {},
    standardsPath: null,
    standardsInline: {},
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
    forceKillGraceSeconds: null,
    requirementsDetected: false,
    managedEnvironmentWarnings: [],
    ...overrides,
  };
}

describe('Skills page', () => {
  beforeEach(() => {
    getConfigMock.mockReset();
    updateConfigMock.mockReset();
    if (typeof window.localStorage.removeItem === 'function') {
      window.localStorage.removeItem('macc.web.skills.customCatalog.v1');
      window.localStorage.removeItem('macc.web.skills.cachedItems.v1');
    }
  });

  it('filters catalog items by search and kind', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    render(<Skills />);

    await screen.findByText('Skills & Catalog');
    expect(screen.getByText('MACC Performer')).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText('Search name, id, tool'), {
      target: { value: 'filesystem' },
    });
    expect(screen.getByText('Filesystem MCP')).toBeInTheDocument();
    expect(screen.queryByText('MACC Performer')).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText('Filter by kind'), { target: { value: 'mcp' } });
    expect(screen.getByText('Filesystem MCP')).toBeInTheDocument();
  });

  it('installs from Add by URL via config update', async () => {
    const initial = buildConfig();
    const saved = buildConfig({ selectedSkills: ['custom-package'] });
    getConfigMock.mockResolvedValue(initial);
    updateConfigMock.mockResolvedValue(saved);

    render(<Skills />);
    await screen.findByText('Skills & Catalog');

    fireEvent.click(screen.getByRole('button', { name: 'Add by URL' }));
    await screen.findByRole('heading', { name: 'Security Review' });

    fireEvent.click(screen.getByRole('button', { name: 'Configuration' }));
    fireEvent.change(screen.getByDisplayValue('custom-package'), {
      target: { value: 'custom-package' },
    });
    fireEvent.change(screen.getByPlaceholderText('https://example.com/package.git'), {
      target: { value: 'https://example.com/custom-package.git' },
    });

    fireEvent.click(screen.getByRole('button', { name: 'Install' }));

    await waitFor(() => {
      expect(updateConfigMock).toHaveBeenCalledTimes(1);
    });

    const payload = updateConfigMock.mock.calls[0][0] as Record<string, unknown>;
    expect(payload.selectedSkills).toEqual(['custom-package']);
  });

  it('removes an installed item with confirmation', async () => {
    const initial = buildConfig({ selectedSkills: ['custom-package'] });
    const saved = buildConfig({ selectedSkills: [] });
    getConfigMock.mockResolvedValue(initial);
    updateConfigMock.mockResolvedValue(saved);

    render(<Skills />);
    await screen.findByText('Skills & Catalog');

    fireEvent.click(screen.getAllByRole('button', { name: 'Remove' })[0]);

    const phraseInput = screen.getByLabelText(/Type/i);
    fireEvent.change(phraseInput, { target: { value: 'custom-package' } });
    fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));

    await waitFor(() => {
      expect(updateConfigMock).toHaveBeenCalledTimes(1);
    });

    const payload = updateConfigMock.mock.calls[0][0] as Record<string, unknown>;
    expect(payload.selectedSkills).toEqual([]);
  });

  it('disables removal for mandatory installed skills', async () => {
    getConfigMock.mockResolvedValue(
      buildConfig({
        selectedSkills: ['macc-performer'],
        mandatorySkills: ['macc-performer'],
      }),
    );

    render(<Skills />);
    await screen.findByText('Skills & Catalog');

    expect(screen.getAllByText('Mandatory').length).toBeGreaterThan(0);
    const removeButton = screen.getAllByRole('button', { name: 'Remove' })[0];
    expect(removeButton).toBeDisabled();
    fireEvent.click(removeButton);
    expect(updateConfigMock).not.toHaveBeenCalled();
  });

  it('renders when selected catalog arrays are missing from the config payload', async () => {
    getConfigMock.mockResolvedValue(
      buildConfig({
        selectedSkills: null as unknown as ApiConfigResponse['selectedSkills'],
        selectedAgents: null as unknown as ApiConfigResponse['selectedAgents'],
        selectedMcp: null as unknown as ApiConfigResponse['selectedMcp'],
        toolPriority: null as unknown as ApiConfigResponse['toolPriority'],
        managedEnvironmentWarnings: null as unknown as ApiConfigResponse['managedEnvironmentWarnings'],
      }),
    );

    render(<Skills />);

    await screen.findByText('Skills & Catalog');
    expect(screen.getByText('MACC Performer')).toBeInTheDocument();
  });
});
