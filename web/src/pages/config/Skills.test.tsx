import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ApiCatalogMcpEntry, ApiCatalogSkillEntry, ApiConfigResponse } from '../../api/models';
import Skills from './Skills';

const getConfigMock = vi.fn();
const updateConfigMock = vi.fn();
const getCatalogSkillsAvailableMock = vi.fn();
const getCatalogMcpAvailableMock = vi.fn();

vi.mock('../../api/client', () => ({
  getConfig: (...args: unknown[]) => getConfigMock(...args),
  updateConfig: (...args: unknown[]) => updateConfigMock(...args),
  getCatalogSkillsAvailable: (...args: unknown[]) => getCatalogSkillsAvailableMock(...args),
  getCatalogMcpAvailable: (...args: unknown[]) => getCatalogMcpAvailableMock(...args),
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

function buildSkill(entry: Partial<ApiCatalogSkillEntry> = {}): ApiCatalogSkillEntry {
  return {
    id: 'macc-performer',
    name: 'MACC Performer',
    description: 'Task-scoped implementation performer for MACC worktrees.',
    tags: ['macc'],
    tools: ['codex'],
    recommended_ref: null,
    risk: null,
    requires_mcp: false,
    writes_user_level_config: false,
    mandatory: false,
    category: null,
    targets: {},
    source: {
      kind: 'git',
      url: 'https://github.com/mod-one/skills.git',
      ref: 'main',
      checksum: null,
    },
    ...entry,
  };
}

function buildMcp(entry: Partial<ApiCatalogMcpEntry> = {}): ApiCatalogMcpEntry {
  return {
    id: 'filesystem-mcp',
    name: 'Filesystem MCP',
    description: 'MCP server for local filesystem browsing and reads.',
    tags: ['mcp'],
    selector: { subpath: 'servers/filesystem' },
    source: {
      kind: 'git',
      url: 'https://example.com/mcp.git',
      ref: 'main',
      checksum: null,
    },
    ...entry,
  };
}

describe('Skills page', () => {
  beforeEach(() => {
    getConfigMock.mockReset();
    updateConfigMock.mockReset();
    getCatalogSkillsAvailableMock.mockReset();
    getCatalogMcpAvailableMock.mockReset();
    getCatalogSkillsAvailableMock.mockResolvedValue({ skills: [buildSkill()] });
    getCatalogMcpAvailableMock.mockResolvedValue({ mcp: [buildMcp()] });
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

  it('installs a catalog skill via config update', async () => {
    const initial = buildConfig();
    const saved = buildConfig({ selectedSkills: ['macc-performer'] });
    getConfigMock.mockResolvedValue(initial);
    updateConfigMock.mockResolvedValue(saved);

    render(<Skills />);
    await screen.findByText('Skills & Catalog');

    fireEvent.click(screen.getAllByRole('button', { name: 'Install' })[0]);

    await waitFor(() => {
      expect(updateConfigMock).toHaveBeenCalledTimes(1);
    });

    const payload = updateConfigMock.mock.calls[0][0] as Record<string, unknown>;
    expect(payload.selectedSkills).toEqual(['macc-performer']);
  });

  it('removes an installed item with confirmation', async () => {
    const initial = buildConfig({ selectedSkills: ['macc-performer'] });
    const saved = buildConfig({ selectedSkills: [] });
    getConfigMock.mockResolvedValue(initial);
    updateConfigMock.mockResolvedValue(saved);

    render(<Skills />);
    await screen.findByText('Skills & Catalog');

    fireEvent.click(screen.getAllByRole('button', { name: 'Remove' })[0]);

    const phraseInput = screen.getByLabelText(/Type/i);
    fireEvent.change(phraseInput, { target: { value: 'macc-performer' } });
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
    getCatalogSkillsAvailableMock.mockResolvedValue({
      skills: [buildSkill({ mandatory: true })],
    });

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
