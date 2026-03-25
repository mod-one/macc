import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ApiConfigResponse } from '../../api/models';
import Settings from './Settings';

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
    coordinatorTool: 'claude',
    referenceBranch: 'main',
    prdFile: 'prd.json',
    taskRegistryFile: null,
    toolPriority: ['claude', 'codex'],
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
    errorCodeRetryList: 'E101,E102',
    errorCodeRetryMax: 2,
    cutoverGateWindowEvents: null,
    cutoverGateMaxBlockedRatio: null,
    cutoverGateMaxStaleRatio: null,
    rateLimitBackoffBaseSeconds: 60,
    rateLimitBackoffMaxSeconds: 3600,
    rateLimitFallbackEnabled: true,
    rateLimitThrottleParallel: true,
    requirementsDetected: false,
    managedEnvironmentWarnings: [],
    ...overrides,
  };
}

function renderPage(): void {
  render(<Settings />);
}

describe('Settings page', () => {
  beforeEach(() => {
    getConfigMock.mockReset();
    updateConfigMock.mockReset();
  });

  it('shows loading spinner then renders General tab by default', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    renderPage();

    expect(screen.getByText('Loading settings...')).toBeInTheDocument();
    await screen.findByText('Settings');
    expect(screen.getByText('Web Port')).toBeInTheDocument();
    expect(screen.getByText('Offline Mode')).toBeInTheDocument();
    expect(screen.getByText('Quiet Mode')).toBeInTheDocument();
  });

  it('shows error banner when config fetch fails', async () => {
    getConfigMock.mockRejectedValue(new Error('Network failure'));
    renderPage();

    await screen.findByText('Network failure');
  });

  it('switches to Coordinator tab and shows coordinator fields', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('button', { name: 'Coordinator' }));

    expect(screen.getByText('Max Dispatch')).toBeInTheDocument();
    expect(screen.getByText('Max Parallel')).toBeInTheDocument();
    expect(screen.getByText('Timeout (seconds)')).toBeInTheDocument();
    expect(screen.getByText('Error Code Retry List')).toBeInTheDocument();
    expect(screen.getByText('Backoff Base (seconds)')).toBeInTheDocument();
  });

  it('switches to Advanced tab and shows raw JSON editor', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('button', { name: 'Advanced' }));

    expect(screen.getByText('Raw Configuration (JSON)')).toBeInTheDocument();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    expect(textarea.value).toContain('"webPort": 3450');
  });

  it('tracks unsaved changes and shows dirty indicator', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    renderPage();

    await screen.findByText('Settings');
    expect(screen.queryByText('You have unsaved changes.')).not.toBeInTheDocument();

    const offlineCheckbox = screen.getByRole('checkbox', { name: /Offline Mode/i });
    fireEvent.click(offlineCheckbox);

    expect(screen.getByText('You have unsaved changes.')).toBeInTheDocument();
  });

  it('discard restores original config', async () => {
    getConfigMock.mockResolvedValue(buildConfig({ offline: false }));
    renderPage();

    await screen.findByText('Settings');
    const offlineCheckbox = screen.getByRole('checkbox', { name: /Offline Mode/i });
    fireEvent.click(offlineCheckbox);

    expect(screen.getByText('You have unsaved changes.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Discard' }));

    expect(screen.queryByText('You have unsaved changes.')).not.toBeInTheDocument();
  });

  it('saves changes via updateConfig and shows success toast', async () => {
    const initial = buildConfig({ offline: false });
    const saved = buildConfig({ offline: true });
    getConfigMock.mockResolvedValue(initial);
    updateConfigMock.mockResolvedValue(saved);
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('checkbox', { name: /Offline Mode/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(updateConfigMock).toHaveBeenCalledTimes(1);
    });
    expect(await screen.findByText('Settings saved')).toBeInTheDocument();
  });

  it('shows error toast when save fails', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    updateConfigMock.mockRejectedValue(new Error('Server error'));
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('checkbox', { name: /Offline Mode/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(await screen.findByText('Failed to save')).toBeInTheDocument();
    expect(screen.getByText('Server error')).toBeInTheDocument();
  });

  it('Advanced tab Apply JSON updates draft', async () => {
    getConfigMock.mockResolvedValue(buildConfig({ webPort: 3450 }));
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('button', { name: 'Advanced' }));

    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    const modified = JSON.parse(textarea.value);
    modified.webPort = 9999;
    fireEvent.change(textarea, { target: { value: JSON.stringify(modified, null, 2) } });
    fireEvent.click(screen.getByRole('button', { name: 'Apply JSON' }));

    // Switch to General tab — the webPort field should reflect the new value
    fireEvent.click(screen.getByRole('button', { name: 'General' }));
    const portInput = screen.getByDisplayValue('9999');
    expect(portInput).toBeInTheDocument();
  });

  it('Advanced tab shows parse error on invalid JSON', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('button', { name: 'Advanced' }));

    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: '{invalid' } });
    fireEvent.click(screen.getByRole('button', { name: 'Apply JSON' }));

    await waitFor(() => {
      const errorBanners = document.querySelectorAll('[class*="error"], [role="alert"]');
      expect(errorBanners.length).toBeGreaterThan(0);
    });
  });
});
