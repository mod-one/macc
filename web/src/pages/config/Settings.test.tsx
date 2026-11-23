import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ApiConfigResponse } from '../../api/models';
import { MemoryRouter } from 'react-router-dom';
import Settings from './Settings';
import { ApiClientError } from '../../api/client';

const getConfigMock = vi.fn();
const updateConfigMock = vi.fn();

vi.mock('../../api/client', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/client')>(),
  getConfig: (...args: unknown[]) => getConfigMock(...args),
  updateConfig: (...args: unknown[]) => updateConfigMock(...args),
}));

function buildConfig(overrides: Partial<ApiConfigResponse> = {}): ApiConfigResponse {
  return {
    revision: 'fixture-revision',
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
    forceKillGraceSeconds: null,
    requirementsDetected: false,
    managedEnvironmentWarnings: [],
    ...overrides,
  };
}

function renderPage(): void {
  render(
    <MemoryRouter initialEntries={['/config/settings']}>
      <Settings />
    </MemoryRouter>,
  );
}

describe('Settings page', () => {
  beforeEach(() => {
    getConfigMock.mockReset();
    updateConfigMock.mockReset();
  });

  it('shows loading spinner then renders Basic tab by default', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    renderPage();

    expect(screen.getByText(/Loading settings/i)).toBeInTheDocument();
    await screen.findByText('Settings');
    expect(screen.getByText('Web port')).toBeInTheDocument();
    expect(screen.getByText('Offline mode')).toBeInTheDocument();
    expect(screen.getByText('Quiet mode')).toBeInTheDocument();
  });

  it('shows error banner when config fetch fails', async () => {
    getConfigMock.mockRejectedValue(new Error('Network failure'));
    renderPage();

    await screen.findByText('Network failure');
  });

  it('switches to Advanced tab and shows advanced fields', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('button', { name: /Advanced/i }));

    expect(screen.getByText('Max dispatch')).toBeInTheDocument();
    expect(screen.getByText('Max Parallel Per Tool')).toBeInTheDocument();
    expect(screen.getByText('Tool Specializations')).toBeInTheDocument();
    expect(screen.getByText('Retry error codes')).toBeInTheDocument();
    expect(screen.getByText('Backoff base (s)')).toBeInTheDocument();
  });

  it('opens the matching tab when navigated from global search', async () => {
    getConfigMock.mockResolvedValue(buildConfig());

    render(
      <MemoryRouter
        initialEntries={[
          {
            pathname: '/config/settings',
            state: { highlightSettingKey: 'maxDispatch' },
          },
        ]}
      >
        <Settings />
      </MemoryRouter>,
    );

    await screen.findByText('Settings');
    expect(screen.getByText('Max dispatch')).toBeInTheDocument();
  });

  it('switches to Raw JSON tab and shows raw JSON editor', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('button', { name: /Raw JSON/i }));

    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    expect(textarea).toBeInTheDocument();
    expect(textarea.value).toContain('"webPort": 3450');
  });

  it('tracks unsaved changes and shows dirty indicator', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    renderPage();

    await screen.findByText('Settings');
    expect(screen.queryByText('Unsaved changes')).not.toBeInTheDocument();

    const offlineCheckbox = screen.getByRole('checkbox', { name: /Offline mode/i });
    fireEvent.click(offlineCheckbox);

    expect(screen.getByText('Unsaved changes')).toBeInTheDocument();
  });

  it('discard restores original config', async () => {
    getConfigMock.mockResolvedValue(buildConfig({ offline: false }));
    renderPage();

    await screen.findByText('Settings');
    const offlineCheckbox = screen.getByRole('checkbox', { name: /Offline mode/i });
    fireEvent.click(offlineCheckbox);

    expect(screen.getByText('Unsaved changes')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Discard' }));

    expect(screen.queryByText('Unsaved changes')).not.toBeInTheDocument();
  });

  it('saves changes via updateConfig and shows success toast', async () => {
    const initial = buildConfig({ offline: false });
    const saved = buildConfig({ offline: true, revision: 'next-revision' });
    getConfigMock.mockResolvedValue(initial);
    updateConfigMock.mockResolvedValue(saved);
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('checkbox', { name: /Offline mode/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() => {
      expect(updateConfigMock).toHaveBeenCalledTimes(1);
    });
    expect(await screen.findByText('Settings saved')).toBeInTheDocument();
    expect(updateConfigMock).toHaveBeenNthCalledWith(1, expect.objectContaining({ revision: initial.revision }));
    fireEvent.click(screen.getByRole('checkbox', { name: /Offline mode/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(updateConfigMock).toHaveBeenNthCalledWith(2, expect.objectContaining({ revision: saved.revision })));

  });

  it('keeps raw edits and uses the loaded revision when another client has saved', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    updateConfigMock.mockRejectedValue(new ApiClientError(409, {
      error: { code: 'config_revision_conflict', category: 'Conflict', message: 'Settings changed', retryable: false },
    }));
    renderPage();
    fireEvent.click(await screen.findByRole('button', { name: /Raw JSON/i }));
    const editor = screen.getByRole('textbox') as HTMLTextAreaElement;
    const fields = JSON.parse(editor.value);
    expect(fields.revision).toBeUndefined();
    fireEvent.change(editor, { target: { value: JSON.stringify({ ...fields, webPort: 5678, revision: 'copied-token' }) } });
    fireEvent.click(screen.getByRole('button', { name: 'Apply JSON' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Your draft is still here');
    expect(JSON.parse(editor.value).webPort).toBe(5678);
    expect(updateConfigMock).toHaveBeenCalledWith(expect.objectContaining({ revision: 'fixture-revision', webPort: 5678 }));
    expect(getConfigMock).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled();
  });

  it('shows error toast when save fails', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    updateConfigMock.mockRejectedValue(new Error('Server error'));
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('checkbox', { name: /Offline mode/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(await screen.findByText('Failed to save')).toBeInTheDocument();
    expect(screen.getByText('Server error')).toBeInTheDocument();
  });

  it('renders when list fields are missing from the config payload', async () => {
    getConfigMock.mockResolvedValue(
      buildConfig({
        enabledTools: null as unknown as ApiConfigResponse['enabledTools'],
        selectedSkills: null as unknown as ApiConfigResponse['selectedSkills'],
        selectedAgents: null as unknown as ApiConfigResponse['selectedAgents'],
        selectedMcp: null as unknown as ApiConfigResponse['selectedMcp'],
        toolPriority: null as unknown as ApiConfigResponse['toolPriority'],
        managedEnvironmentWarnings: null as unknown as ApiConfigResponse['managedEnvironmentWarnings'],
      }),
    );
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('button', { name: /Advanced/i }));
    expect(screen.getByText('PRD file')).toBeInTheDocument();
  });

  it('lets coordinator tool settings update the draft', async () => {
    getConfigMock.mockResolvedValue(buildConfig({ enabledTools: ['claude', 'codex'] }));
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('button', { name: /Advanced/i }));

    const increaseBtn = screen.getAllByRole('button', { name: 'Increase' })[0];
    fireEvent.click(increaseBtn);

    expect(screen.getByText('Unsaved changes')).toBeInTheDocument();
  });

  it('saves zero and restores the global limit by removing only the selected override', async () => {
    const config = buildConfig({ enabledTools: ['tool-a', 'tool-b'], maxParallelPerTool: { 'tool-b': 3 } });
    const caps = { 'tool-a': 0, 'tool-b': 3 };
    getConfigMock.mockResolvedValue(config);
    updateConfigMock.mockResolvedValue({ ...config, maxParallelPerTool: caps });
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('button', { name: /Advanced/i }));
    expect(screen.getByText('Global limit')).toBeInTheDocument();
    const decrease = screen.getAllByRole('button', { name: 'Decrease' })[0];
    fireEvent.click(decrease);
    expect(decrease).toBeDisabled();
    expect(screen.getByText('No new assignments')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() => expect(updateConfigMock).toHaveBeenCalledWith(
      expect.objectContaining({ maxParallelPerTool: caps }),
    ));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled());
    fireEvent.click(screen.getAllByRole('button', { name: 'Use global limit' })[0]);
    expect(screen.getByText('Global limit')).toBeInTheDocument();
    updateConfigMock.mockResolvedValue(config);
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(updateConfigMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ maxParallelPerTool: { 'tool-b': 3 } }),
    ));
  });

  it('Advanced tab Apply JSON updates draft', async () => {
    getConfigMock.mockResolvedValue(buildConfig({ webPort: 3450 }));
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('button', { name: /Raw JSON/i }));

    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    const modified = JSON.parse(textarea.value);
    modified.webPort = 9999;
    fireEvent.change(textarea, { target: { value: JSON.stringify(modified, null, 2) } });
    fireEvent.click(screen.getByRole('button', { name: 'Apply JSON' }));

    // Switch to Basic tab — the webPort field should reflect the new value
    fireEvent.click(screen.getByRole('button', { name: /Basic/i }));
    const portInput = screen.getByDisplayValue('9999');
    expect(portInput).toBeInTheDocument();
  });

  it('Advanced tab shows parse error on invalid JSON', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    renderPage();

    await screen.findByText('Settings');
    fireEvent.click(screen.getByRole('button', { name: /Raw JSON/i }));

    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: '{invalid' } });
    fireEvent.click(screen.getByRole('button', { name: 'Apply JSON' }));

    await waitFor(() => {
      const errorBanners = document.querySelectorAll('[class*="error"], [role="alert"]');
      expect(errorBanners.length).toBeGreaterThan(0);
    });
  });
});
