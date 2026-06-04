import React from 'react';
import { useNavigate } from 'react-router-dom';
import { ApiClientError, getConfig, getStandardsPreview, runPlan, updateConfig } from '../api/client';
import type {
  ApiConfigResponse,
  ApiPlanResponse,
  ApiStandardsPreviewCard,
  ApiStandardsPreviewRequest,
  JsonValue,
} from '../api/models';
import { Button, ErrorBanner, LoadingSpinner } from '../components';
import { CheckIcon, AlertTriangleIcon } from '../components/icons';
import { cn } from '../components/styles';

type InitStep = 0 | 1 | 2 | 3;
type InitPresetId = 'minimal' | 'strict' | 'none';
type ToolHealth = 'healthy' | 'degraded';

interface InitPreset {
  id: InitPresetId;
  label: string;
  description: string;
  values: Record<string, string>;
}

interface ToolOption {
  id: string;
  version: string;
  health: ToolHealth;
  enabled: boolean;
}

interface ConfigWithRootHint extends ApiConfigResponse {
  projectRoot?: string | null;
  root?: string | null;
  cwd?: string | null;
}

const STEPS = ['Project root', 'Tool detection', 'Standards', 'Review'] as const;

const STEP_DESCRIPTIONS = [
  'Confirm the project root before writing .macc/macc.yaml.',
  'Choose which tool adapters to enable for this project.',
  'Select a standards preset and preview the generated output.',
  'Review the configuration before saving.',
] as const;

const PRESETS: InitPreset[] = [
  {
    id: 'minimal',
    label: 'Minimal',
    description: 'English language and pnpm defaults.',
    values: {
      language: 'English',
      package_manager: 'pnpm',
    },
  },
  {
    id: 'strict',
    label: 'Strict',
    description: 'Minimal defaults with stricter TypeScript and import guidance.',
    values: {
      language: 'English',
      package_manager: 'pnpm',
      typescript: 'strict',
      imports: 'absolute:@/',
    },
  },
  {
    id: 'none',
    label: 'None',
    description: 'Start with an empty standards set.',
    values: {},
  },
];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isJsonObject(value: JsonValue | undefined): value is Record<string, JsonValue> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function ensureJsonRecord(value: unknown): Record<string, JsonValue> {
  return isJsonObject(value as JsonValue | undefined) ? (value as Record<string, JsonValue>) : {};
}

function ensureStringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === 'string') : [];
}

function normalizeConfigResponse(config: ApiConfigResponse): ApiConfigResponse {
  return {
    ...config,
    enabledTools: ensureStringArray(config.enabledTools),
    toolConfig: ensureJsonRecord(config.toolConfig),
    toolSettings: ensureJsonRecord(config.toolSettings),
    toolPriority: ensureStringArray(config.toolPriority),
    standardsInline: isRecord(config.standardsInline)
      ? Object.fromEntries(
          Object.entries(config.standardsInline).filter(
            (entry): entry is [string, string] => typeof entry[1] === 'string',
          ),
        )
      : {},
    selectedSkills: ensureStringArray(config.selectedSkills),
    selectedAgents: ensureStringArray(config.selectedAgents),
    selectedMcp: ensureStringArray(config.selectedMcp),
    managedEnvironmentWarnings: ensureStringArray(config.managedEnvironmentWarnings),
  };
}

function formatError(error: unknown): string {
  if (error instanceof ApiClientError) {
    return `${error.envelope.error.message} (${error.envelope.error.code})`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return 'Unexpected initialization error.';
}

function titleCase(value: string): string {
  return value
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}

function normalizePath(input: string): string {
  const trimmed = input.trim();
  return trimmed.length > 0 ? trimmed : '';
}

function asString(value: JsonValue | undefined): string | null {
  if (typeof value === 'string') {
    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : null;
  }
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  return null;
}

function asBoolean(value: JsonValue | undefined): boolean | null {
  return typeof value === 'boolean' ? value : null;
}

function collectToolIds(config: ApiConfigResponse): string[] {
  const ids = new Set<string>();
  for (const id of config.enabledTools) ids.add(id);
  for (const id of Object.keys(config.toolConfig)) ids.add(id);
  for (const id of Object.keys(config.toolSettings)) ids.add(id);
  for (const id of config.toolPriority) ids.add(id);
  return Array.from(ids).sort((left, right) => left.localeCompare(right));
}

function buildToolOptions(config: ApiConfigResponse): ToolOption[] {
  const enabled = new Set(config.enabledTools);

  return collectToolIds(config).map((toolId) => {
    const configEntry = isJsonObject(config.toolConfig[toolId]) ? config.toolConfig[toolId] : {};
    const settingsEntry = isJsonObject(config.toolSettings[toolId]) ? config.toolSettings[toolId] : {};

    const version =
      asString(configEntry.version) ??
      asString(settingsEntry.version) ??
      asString(configEntry.adapterVersion) ??
      asString(settingsEntry.adapterVersion) ??
      'n/a';

    const healthLabel =
      asString(settingsEntry.health) ??
      asString(configEntry.health) ??
      asString(settingsEntry.status) ??
      asString(configEntry.status) ??
      '';
    const health =
      asBoolean(settingsEntry.healthy) === false ||
      asBoolean(configEntry.healthy) === false ||
      /(degraded|error|failed|unhealthy)/i.test(healthLabel)
        ? 'degraded'
        : 'healthy';

    return {
      id: toolId,
      version,
      health,
      enabled: enabled.has(toolId),
    };
  });
}

function buildStandardsInline(presetId: InitPresetId): Record<string, string> {
  return { ...PRESETS.find((preset) => preset.id === presetId)?.values };
}

function yamlScalar(value: unknown): string {
  if (value === null || typeof value === 'undefined') return 'null';
  if (typeof value === 'string') return JSON.stringify(value);
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return JSON.stringify(value);
}

function yamlLines(value: unknown, indent = 0): string[] {
  const pad = '  '.repeat(indent);

  if (Array.isArray(value)) {
    if (value.length === 0) return [`${pad}[]`];
    return value.flatMap((entry) => {
      if (entry !== null && typeof entry === 'object' && !Array.isArray(entry)) {
        const nested = yamlLines(entry, indent + 1);
        return [`${pad}-`, ...nested];
      }
      return [`${pad}- ${yamlScalar(entry)}`];
    });
  }

  if (value !== null && typeof value === 'object') {
    const entries = Object.entries(value);
    if (entries.length === 0) return [`${pad}{}`];
    return entries.flatMap(([key, entry]) => {
      if (entry === null || typeof entry !== 'object' || Array.isArray(entry)) {
        return [`${pad}${key}: ${yamlScalar(entry)}`];
      }
      const nested = yamlLines(entry, indent + 1);
      return [`${pad}${key}:`, ...nested];
    });
  }

  return [`${pad}${yamlScalar(value)}`];
}

function toYaml(value: unknown): string {
  return yamlLines(value).join('\n');
}

function isStandardsPreviewCard(value: unknown): value is ApiStandardsPreviewCard {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.title === 'string' &&
    typeof value.content === 'string'
  );
}

const Init: React.FC = () => {
  const navigate = useNavigate();

  const [isLoading, setIsLoading] = React.useState(true);
  const [isSaving, setIsSaving] = React.useState(false);
  const [loadError, setLoadError] = React.useState<string | null>(null);
  const [saveError, setSaveError] = React.useState<string | null>(null);

  const [config, setConfig] = React.useState<ApiConfigResponse | null>(null);
  const [projectRoot, setProjectRoot] = React.useState('');
  const [selectedTools, setSelectedTools] = React.useState<Set<string>>(new Set());
  const [presetId, setPresetId] = React.useState<InitPresetId>('minimal');
  const [reviewAccepted, setReviewAccepted] = React.useState(false);

  const [previewCards, setPreviewCards] = React.useState<ApiStandardsPreviewCard[]>([]);
  const [isPreviewLoading, setIsPreviewLoading] = React.useState(false);
  const [previewError, setPreviewError] = React.useState<string | null>(null);
  const [planPreview, setPlanPreview] = React.useState<ApiPlanResponse | null>(null);
  const [isPlanPreviewLoading, setIsPlanPreviewLoading] = React.useState(false);
  const [planPreviewError, setPlanPreviewError] = React.useState<string | null>(null);

  const [step, setStep] = React.useState<InitStep>(0);
  const [stepError, setStepError] = React.useState<string | null>(null);

  const detectedRoot = React.useMemo(() => {
    if (!config) return '';
    const hinted =
      (config as ConfigWithRootHint).projectRoot ??
      (config as ConfigWithRootHint).root ??
      (config as ConfigWithRootHint).cwd;
    return normalizePath(hinted ?? '.') || '.';
  }, [config]);

  const toolOptions = React.useMemo(() => (config ? buildToolOptions(config) : []), [config]);
  const standardsInline = React.useMemo(() => buildStandardsInline(presetId), [presetId]);

  const previewConfig = React.useMemo(() => {
    if (!config) return null;
    return {
      ...config,
      enabledTools: Array.from(selectedTools).sort((left, right) => left.localeCompare(right)),
      standardsPath: null,
      standardsInline,
    };
  }, [config, selectedTools, standardsInline]);

  const progressPercent = ((step + 1) / STEPS.length) * 100;

  const validateStep = React.useCallback(
    (candidateStep: InitStep): string | null => {
      if (candidateStep === 0) {
        if (projectRoot.trim().length === 0) return 'Project root is required.';
        if (normalizePath(projectRoot).length === 0) return 'Project root cannot be blank.';
      }
      if (candidateStep === 1 && selectedTools.size === 0) {
        return 'Enable at least one tool before continuing.';
      }
      if (candidateStep === 2 && !PRESETS.some((preset) => preset.id === presetId)) {
        return 'Choose a standards preset.';
      }
      if (candidateStep === 3 && !reviewAccepted) {
        return 'Review and accept the configuration before finishing.';
      }
      return null;
    },
    [presetId, projectRoot, reviewAccepted, selectedTools.size],
  );

  React.useEffect(() => {
    let cancelled = false;
    void (async () => {
      setIsLoading(true);
      try {
        const nextConfig = normalizeConfigResponse(await getConfig());
        if (cancelled) return;
        const inferredRoot = normalizePath(
          (
            (nextConfig as ConfigWithRootHint).projectRoot ??
            (nextConfig as ConfigWithRootHint).root ??
            (nextConfig as ConfigWithRootHint).cwd ??
            '.'
          ).toString(),
        );
        setConfig(nextConfig);
        setProjectRoot(inferredRoot || '.');
        setSelectedTools(
          new Set(
            nextConfig.enabledTools.length > 0
              ? nextConfig.enabledTools
              : collectToolIds(nextConfig),
          ),
        );
        setPresetId('minimal');
        setReviewAccepted(false);
        setStep(0);
        setStepError(null);
        setSaveError(null);
        setPreviewError(null);
        setPreviewCards([]);
      } catch (error) {
        if (!cancelled) setLoadError(formatError(error));
      } finally {
        if (!cancelled) setIsLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, []);

  React.useEffect(() => {
    if (!config) return;
    let cancelled = false;
    setIsPreviewLoading(true);
    setPreviewError(null);
    const request: ApiStandardsPreviewRequest = { standardsPath: null, standardsInline };
    void getStandardsPreview(request)
      .then((response) => {
        if (cancelled) return;
        setPreviewCards(response.cards.filter(isStandardsPreviewCard));
      })
      .catch((error) => {
        if (!cancelled) setPreviewError(formatError(error));
      })
      .finally(() => {
        if (!cancelled) setIsPreviewLoading(false);
      });
    return () => { cancelled = true; };
  }, [config, standardsInline]);

  const goNext = React.useCallback(() => {
    const issue = validateStep(step);
    if (issue) { setStepError(issue); return; }
    setStepError(null);
    setStep((current) => Math.min(current + 1, 3) as InitStep);
  }, [step, validateStep]);

  const goBack = React.useCallback(() => {
    setStepError(null);
    setStep((current) => Math.max(current - 1, 0) as InitStep);
  }, []);

  const useDefaults = React.useCallback(() => {
    if (!config) return;
    setProjectRoot(detectedRoot || '.');
    setSelectedTools(
      new Set(
        config.enabledTools.length > 0 ? config.enabledTools : collectToolIds(config),
      ),
    );
    setPresetId('minimal');
    setReviewAccepted(false);
    setPlanPreview(null);
    setPlanPreviewError(null);
    setStepError(null);
    setStep(3);
  }, [config, detectedRoot]);

  const toggleTool = React.useCallback((toolId: string) => {
    setSelectedTools((current) => {
      const next = new Set(current);
      if (next.has(toolId)) { next.delete(toolId); } else { next.add(toolId); }
      return next;
    });
    setStepError(null);
  }, []);

  const handlePlanPreview = React.useCallback(async () => {
    setIsPlanPreviewLoading(true);
    setPlanPreviewError(null);
    try {
      const response = await runPlan({
        scope: 'project',
        tools: Array.from(selectedTools),
        allowUserScope: false,
        includeDiff: true,
        explain: true,
      });
      setPlanPreview(response);
    } catch (error) {
      setPlanPreviewError(formatError(error));
    } finally {
      setIsPlanPreviewLoading(false);
    }
  }, [selectedTools]);

  const handleFinish = React.useCallback(async () => {
    for (const candidateStep of [0, 1, 2, 3] as const) {
      const issue = validateStep(candidateStep);
      if (issue) { setStep(candidateStep); setStepError(issue); return; }
    }
    if (!config) return;
    setIsSaving(true);
    setSaveError(null);
    try {
      await updateConfig({
        enabledTools: Array.from(selectedTools).sort((left, right) => left.localeCompare(right)),
        toolConfig: config.toolConfig,
        toolSettings: config.toolSettings,
        standardsPath: null,
        standardsInline,
      });
      await navigate('/dashboard');
    } catch (error) {
      setSaveError(formatError(error));
    } finally {
      setIsSaving(false);
    }
  }, [config, navigate, selectedTools, standardsInline, validateStep]);

  const renderStepContent = (): React.ReactNode => {
    // Step 0: Project root
    if (step === 0) {
      return (
        <div
          className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
          style={{ boxShadow: 'var(--shadow-soft)' }}
        >
          <div className="px-5 py-5">
            <label className="block">
              <span className="text-sm font-medium text-[var(--text-primary)]">Project root</span>
              <input
                aria-label="Project root"
                className="mt-2 h-10 w-full rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-3 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50 transition-shadow"
                onChange={(event) => { setProjectRoot(event.target.value); setStepError(null); }}
                placeholder={detectedRoot || '.'}
                value={projectRoot}
              />
              {detectedRoot && (
                <p className="mt-1.5 text-xs text-[var(--text-muted)]">
                  Detected from backend:{' '}
                  <span className="font-mono">{detectedRoot}</span>
                </p>
              )}
            </label>
          </div>
          <div className="border-t border-[var(--border-subtle)] bg-[var(--bg-secondary)] px-5 py-3 text-xs text-[var(--text-secondary)]">
            This path determines where{' '}
            <span className="font-mono">.macc/macc.yaml</span> will be written. Relative paths are resolved from the current working directory.
          </div>
        </div>
      );
    }

    // Step 1: Tool detection
    if (step === 1) {
      return (
        <div
          className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
          style={{ boxShadow: 'var(--shadow-soft)' }}
        >
          {toolOptions.length === 0 ? (
            <p className="px-5 py-8 text-center text-sm text-[var(--text-secondary)]">
              No tool adapters detected. Continue with defaults or return after installing adapters.
            </p>
          ) : (
            <ul className="divide-y divide-[var(--border-subtle)]">
              {toolOptions.map((tool) => (
                <li key={tool.id}>
                  <label className="flex cursor-pointer items-center gap-4 px-4 py-3 transition-colors hover:bg-[var(--bg-elevated)]">
                    <div className="min-w-0 flex-1">
                      <span className="text-sm font-medium text-[var(--text-primary)]">
                        {titleCase(tool.id)}
                      </span>
                      {tool.version !== 'n/a' && (
                        <span className="ml-2 font-mono text-[11px] text-[var(--text-muted)]">
                          v{tool.version}
                        </span>
                      )}
                    </div>
                    <span
                      className="flex shrink-0 items-center gap-1.5 text-[11px]"
                      style={{
                        color:
                          tool.health === 'healthy' ? 'var(--success)' : 'var(--warning)',
                      }}
                    >
                      <span
                        className="h-1.5 w-1.5 rounded-full"
                        style={{
                          backgroundColor:
                            tool.health === 'healthy' ? 'var(--success)' : 'var(--warning)',
                        }}
                      />
                      {tool.health}
                    </span>
                    {/* Visual toggle switch */}
                    <span
                      aria-hidden
                      className="relative inline-flex h-5 w-9 shrink-0 items-center rounded-full border-2 border-transparent transition-colors"
                      style={{
                        backgroundColor: selectedTools.has(tool.id)
                          ? 'var(--accent)'
                          : 'var(--border)',
                      }}
                    >
                      <span
                        className="inline-block h-4 w-4 rounded-full bg-white shadow-sm transition-transform"
                        style={{
                          transform: selectedTools.has(tool.id)
                            ? 'translateX(1rem)'
                            : 'translateX(0)',
                        }}
                      />
                    </span>
                    <input
                      aria-label={`Enable ${tool.id}`}
                      checked={selectedTools.has(tool.id)}
                      className="sr-only"
                      onChange={() => toggleTool(tool.id)}
                      type="checkbox"
                    />
                  </label>
                </li>
              ))}
            </ul>
          )}
          <div className="border-t border-[var(--border-subtle)] bg-[var(--bg-secondary)] px-5 py-2.5 text-xs text-[var(--text-muted)]">
            {selectedTools.size} of {toolOptions.length} adapters enabled
          </div>
        </div>
      );
    }

    // Step 2: Standards
    if (step === 2) {
      return (
        <div className="grid gap-5 lg:grid-cols-[0.85fr_1.15fr]">
          {/* Preset selector */}
          <fieldset className="min-w-0">
            <legend className="mb-2 text-sm font-medium text-[var(--text-primary)]">
              Standards preset
            </legend>
            <div
              className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
              style={{ boxShadow: 'var(--shadow-soft)' }}
            >
              {PRESETS.map((preset, index) => (
                <label
                  key={preset.id}
                  className={cn(
                    'flex cursor-pointer items-center gap-3 px-4 py-3 transition-colors',
                    index > 0 && 'border-t border-[var(--border-subtle)]',
                    preset.id === presetId
                      ? 'bg-[var(--accent-bg)]'
                      : 'hover:bg-[var(--bg-elevated)]',
                  )}
                >
                  <input
                    checked={preset.id === presetId}
                    className="h-4 w-4 accent-[var(--accent)] shrink-0"
                    name="standards-preset"
                    onChange={() => { setPresetId(preset.id); setStepError(null); }}
                    type="radio"
                    value={preset.id}
                  />
                  <div className="min-w-0 flex-1">
                    <p
                      className={cn(
                        'text-sm font-medium',
                        preset.id === presetId
                          ? 'text-[var(--text-primary)]'
                          : 'text-[var(--text-secondary)]',
                      )}
                    >
                      {preset.label}
                    </p>
                    <p className="mt-0.5 text-xs text-[var(--text-muted)]">
                      {preset.description}
                    </p>
                  </div>
                  {preset.id === presetId && (
                    <CheckIcon
                      className="h-3.5 w-3.5 shrink-0"
                      style={{ color: 'var(--accent)' }}
                    />
                  )}
                </label>
              ))}
            </div>
          </fieldset>

          {/* Preview */}
          <div
            className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
            style={{ boxShadow: 'var(--shadow-soft)' }}
          >
            <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-2.5">
              <span className="text-sm font-medium text-[var(--text-primary)]">
                Standards preview
              </span>
              {isPreviewLoading && (
                <span className="text-xs text-[var(--text-muted)]">Refreshing...</span>
              )}
            </div>

            {previewError && (
              <div className="border-b border-[var(--error)]/30 bg-[var(--error)]/10 px-4 py-2.5 text-xs text-[var(--text-primary)]">
                Preview refresh failed: {previewError}
              </div>
            )}

            {previewCards.length > 0 ? (
              <div className="divide-y divide-[var(--border-subtle)]">
                {previewCards.map((card) => (
                  <div key={card.id} className="px-4 py-3">
                    <p className="text-xs font-semibold text-[var(--text-primary)]">
                      {card.title}
                    </p>
                    <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px] leading-5 text-[var(--text-secondary)]">
                      {card.content}
                    </pre>
                  </div>
                ))}
              </div>
            ) : !isPreviewLoading ? (
              <p className="px-4 py-6 text-sm text-[var(--text-secondary)]">
                No preview returned yet.
              </p>
            ) : null}
          </div>
        </div>
      );
    }

    // Step 3: Review
    return (
      <div className="grid gap-5 lg:grid-cols-[1.1fr_0.9fr]">
        {/* YAML preview */}
        <div
          className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
          style={{ boxShadow: 'var(--shadow-soft)' }}
        >
          <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-2.5">
            <span className="text-sm font-medium text-[var(--text-primary)]">Config preview</span>
            <Button
              className="h-7 border-[var(--border)] bg-[var(--bg-secondary)] px-2.5 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
              disabled={isPlanPreviewLoading}
              onClick={handlePlanPreview}
              type="button"
            >
              {isPlanPreviewLoading ? 'Loading plan...' : 'Preview plan'}
            </Button>
          </div>
          <pre className="max-h-[32rem] overflow-auto p-4 font-mono text-[11px] leading-5 text-[var(--text-secondary)]">
            {previewConfig ? toYaml(previewConfig) : 'Loading preview...'}
          </pre>
        </div>

        {/* Summary + plan */}
        <div className="flex flex-col gap-4">
          {/* Summary */}
          <div
            className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
            style={{ boxShadow: 'var(--shadow-soft)' }}
          >
            <div className="border-b border-[var(--border)] px-4 py-2.5">
              <span className="text-sm font-medium text-[var(--text-primary)]">Summary</span>
            </div>
            <dl className="divide-y divide-[var(--border-subtle)]">
              <div className="flex items-center justify-between gap-4 px-4 py-2.5 text-sm">
                <dt className="text-[var(--text-secondary)]">Project root</dt>
                <dd className="truncate font-mono text-[13px] text-[var(--text-primary)]">
                  {projectRoot || detectedRoot || '.'}
                </dd>
              </div>
              <div className="flex items-center justify-between gap-4 px-4 py-2.5 text-sm">
                <dt className="text-[var(--text-secondary)]">Enabled tools</dt>
                <dd className="font-semibold tabular-nums text-[var(--text-primary)]">
                  {selectedTools.size}
                </dd>
              </div>
              <div className="flex items-center justify-between gap-4 px-4 py-2.5 text-sm">
                <dt className="text-[var(--text-secondary)]">Standards preset</dt>
                <dd className="text-[var(--text-primary)]">
                  {PRESETS.find((p) => p.id === presetId)?.label ?? presetId}
                </dd>
              </div>
            </dl>
          </div>

          {/* Optional plan */}
          {(planPreview || planPreviewError) && (
            <div
              className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]"
              style={{ boxShadow: 'var(--shadow-soft)' }}
            >
              <div className="border-b border-[var(--border)] px-4 py-2.5">
                <span className="text-sm font-medium text-[var(--text-primary)]">Plan preview</span>
              </div>

              {planPreviewError && (
                <p className="px-4 py-3 text-xs text-[var(--text-secondary)]">
                  {planPreviewError}
                </p>
              )}

              {planPreview && (
                <div className="divide-y divide-[var(--border-subtle)]">
                  <div className="px-4 py-3">
                    <dl className="space-y-1.5 text-sm">
                      <div className="flex justify-between gap-4">
                        <dt className="text-[var(--text-secondary)]">Total actions</dt>
                        <dd className="tabular-nums text-[var(--text-primary)]">
                          {planPreview.summary.totalActions}
                        </dd>
                      </div>
                      <div className="flex justify-between gap-4">
                        <dt className="text-[var(--text-secondary)]">Files to write</dt>
                        <dd className="tabular-nums text-[var(--text-primary)]">
                          {planPreview.summary.filesWrite}
                        </dd>
                      </div>
                      <div className="flex justify-between gap-4">
                        <dt className="text-[var(--text-secondary)]">Files to merge</dt>
                        <dd className="tabular-nums text-[var(--text-primary)]">
                          {planPreview.summary.filesMerge}
                        </dd>
                      </div>
                    </dl>
                  </div>
                  {planPreview.risks.length > 0 && (
                    <div className="px-4 py-3">
                      <p className="mb-2 text-xs font-medium text-[var(--text-secondary)]">
                        Risks
                      </p>
                      <ul className="space-y-1.5">
                        {planPreview.risks.map((risk) => (
                          <li
                            key={`${risk.level}-${risk.message}`}
                            className="flex items-start gap-2 text-sm text-[var(--text-secondary)]"
                          >
                            <AlertTriangleIcon
                              className="mt-0.5 h-3.5 w-3.5 shrink-0"
                              style={{ color: 'var(--warning)' }}
                            />
                            {risk.message}
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    );
  };

  if (isLoading) {
    return (
      <div className="flex min-h-[48vh] items-center justify-center">
        <LoadingSpinner label="Loading setup wizard" />
      </div>
    );
  }

  if (loadError) {
    return (
      <div className="flex flex-col gap-5 pb-8">
        <div>
          <h1 className="text-xl font-semibold text-[var(--text-primary)]">Setup wizard</h1>
          <p className="mt-0.5 text-sm text-[var(--text-secondary)]">
            Failed to load project configuration.
          </p>
        </div>
        <ErrorBanner title="Unable to load setup wizard" message={loadError} />
      </div>
    );
  }

  return (
    <div className="flex max-w-4xl flex-col gap-5 pb-8">
      {/* Header: step name + progress */}
      <div>
        <div className="flex items-start justify-between gap-3">
          <div>
            <h1 className="text-xl font-semibold text-[var(--text-primary)]">
              {STEPS[step]}
            </h1>
            <p className="mt-0.5 text-sm text-[var(--text-secondary)]">
              {STEP_DESCRIPTIONS[step]}
            </p>
          </div>
          <span className="shrink-0 text-sm text-[var(--text-muted)]">
            {step + 1} of {STEPS.length}
          </span>
        </div>

        {/* Progress bar + step labels */}
        <div className="mt-4">
          <div className="h-0.5 overflow-hidden rounded-full bg-[var(--bg-elevated)]">
            <div
              className="h-full rounded-full bg-[var(--accent)] transition-[width] duration-300"
              style={{ width: `${progressPercent}%` }}
            />
          </div>
          <ol className="mt-2.5 flex items-center" aria-label="Setup progress">
            {STEPS.map((label, index) => (
              <React.Fragment key={label}>
                <li
                  className={cn(
                    'whitespace-nowrap text-xs transition-colors',
                    index === step ? 'font-medium' : '',
                  )}
                  style={{
                    color:
                      index === step
                        ? 'var(--accent)'
                        : index < step
                          ? 'var(--success)'
                          : 'var(--text-muted)',
                  }}
                >
                  {label}
                </li>
                {index < STEPS.length - 1 && (
                  <span
                    aria-hidden
                    className="mx-2 h-px min-w-4 flex-1 rounded-full"
                    style={{
                      backgroundColor:
                        index < step ? 'var(--success)' : 'var(--border)',
                    }}
                  />
                )}
              </React.Fragment>
            ))}
          </ol>
        </div>
      </div>

      {/* Errors */}
      {stepError && <ErrorBanner title="Validation required" message={stepError} />}
      {saveError && <ErrorBanner title="Initialization failed" message={saveError} />}

      {/* Step content */}
      {renderStepContent()}

      {/* Navigation */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-t border-[var(--border-subtle)] pt-4">
        <Button
          className="border-[var(--border)] bg-[var(--bg-card)]"
          disabled={step === 0 || isSaving}
          onClick={goBack}
          type="button"
        >
          Back
        </Button>
        <div className="flex flex-wrap items-center gap-3">
          <Button
            className="border-[var(--border)] bg-[var(--bg-card)] text-[var(--text-secondary)]"
            onClick={useDefaults}
            type="button"
          >
            Skip to defaults
          </Button>
          {step < 3 ? (
            <Button
              className="border-transparent bg-[var(--accent)] text-white hover:brightness-110"
              onClick={goNext}
              type="button"
            >
              Continue
            </Button>
          ) : (
            <>
              <label className="flex cursor-pointer items-center gap-2 text-sm text-[var(--text-secondary)]">
                <input
                  checked={reviewAccepted}
                  className="h-4 w-4 accent-[var(--accent)]"
                  onChange={(event) => {
                    setReviewAccepted(event.target.checked);
                    setStepError(null);
                  }}
                  type="checkbox"
                />
                I reviewed the configuration
              </label>
              <Button
                className="border-transparent bg-[var(--accent)] text-white hover:brightness-110"
                disabled={isSaving}
                onClick={handleFinish}
                type="button"
              >
                {isSaving ? 'Creating project...' : 'Create project'}
              </Button>
            </>
          )}
        </div>
      </div>
    </div>
  );
};

export default Init;
