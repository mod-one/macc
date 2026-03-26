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
import { Button, ErrorBanner, LoadingSpinner, StatusBadge } from '../components';
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

const STEPS = ['Welcome & Project Root', 'Tool Detection', 'Standards', 'Review'] as const;

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
  if (value === null || typeof value === 'undefined') {
    return 'null';
  }
  if (typeof value === 'string') {
    return JSON.stringify(value);
  }
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  return JSON.stringify(value);
}

function yamlLines(value: unknown, indent = 0): string[] {
  const pad = '  '.repeat(indent);

  if (Array.isArray(value)) {
    if (value.length === 0) {
      return [`${pad}[]`];
    }

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
    if (entries.length === 0) {
      return [`${pad}{}`];
    }

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
  return isRecord(value) && typeof value.id === 'string' && typeof value.title === 'string' && typeof value.content === 'string';
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
    if (!config) {
      return '';
    }
    const hinted = (config as ConfigWithRootHint).projectRoot ?? (config as ConfigWithRootHint).root ?? (config as ConfigWithRootHint).cwd;
    return normalizePath(hinted ?? '.') || '.';
  }, [config]);

  const toolOptions = React.useMemo(() => (config ? buildToolOptions(config) : []), [config]);

  const standardsInline = React.useMemo(() => buildStandardsInline(presetId), [presetId]);

  const previewConfig = React.useMemo(() => {
    if (!config) {
      return null;
    }

    return {
      ...config,
      enabledTools: Array.from(selectedTools).sort((left, right) => left.localeCompare(right)),
      standardsPath: null,
      standardsInline,
    };
  }, [config, selectedTools, standardsInline]);

  const stepIndexLabel = `${step + 1} / ${STEPS.length}`;
  const progressPercent = ((step + 1) / STEPS.length) * 100;

  const validateStep = React.useCallback(
    (candidateStep: InitStep): string | null => {
      if (candidateStep === 0) {
        if (projectRoot.trim().length === 0) {
          return 'Project root is required.';
        }
        if (normalizePath(projectRoot).length === 0) {
          return 'Project root cannot be blank.';
        }
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
        if (cancelled) {
          return;
        }

        const inferredRoot = normalizePath(
          ((nextConfig as ConfigWithRootHint).projectRoot ?? (nextConfig as ConfigWithRootHint).root ?? (nextConfig as ConfigWithRootHint).cwd ?? '.')
            .toString(),
        );

        setConfig(nextConfig);
        setProjectRoot(inferredRoot || '.');
        setSelectedTools(new Set(nextConfig.enabledTools.length > 0 ? nextConfig.enabledTools : collectToolIds(nextConfig)));
        setPresetId('minimal');
        setReviewAccepted(false);
        setStep(0);
        setStepError(null);
        setSaveError(null);
        setPreviewError(null);
        setPreviewCards([]);
      } catch (error) {
        if (!cancelled) {
          setLoadError(formatError(error));
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  React.useEffect(() => {
    if (!config) {
      return;
    }

    let cancelled = false;
    setIsPreviewLoading(true);
    setPreviewError(null);

    const request: ApiStandardsPreviewRequest = {
      standardsPath: null,
      standardsInline,
    };

    void getStandardsPreview(request)
      .then((response) => {
        if (cancelled) {
          return;
        }
        setPreviewCards(response.cards.filter(isStandardsPreviewCard));
      })
      .catch((error) => {
        if (!cancelled) {
          setPreviewError(formatError(error));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setIsPreviewLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [config, standardsInline]);

  const goNext = React.useCallback(() => {
    const issue = validateStep(step);
    if (issue) {
      setStepError(issue);
      return;
    }
    setStepError(null);
    setStep((current) => Math.min(current + 1, 3) as InitStep);
  }, [step, validateStep]);

  const goBack = React.useCallback(() => {
    setStepError(null);
    setStep((current) => Math.max(current - 1, 0) as InitStep);
  }, []);

  const useDefaults = React.useCallback(() => {
    if (!config) {
      return;
    }
    setProjectRoot(detectedRoot || '.');
    setSelectedTools(new Set(config.enabledTools.length > 0 ? config.enabledTools : collectToolIds(config)));
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
      if (next.has(toolId)) {
        next.delete(toolId);
      } else {
        next.add(toolId);
      }
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
      if (issue) {
        setStep(candidateStep);
        setStepError(issue);
        return;
      }
    }

    if (!config) {
      return;
    }

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
    if (step === 0) {
      return (
        <div className="grid gap-6 lg:grid-cols-[1.2fr_0.8fr]">
          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] p-5">
            <h2 className="text-lg font-semibold text-[var(--text-primary)]">Welcome</h2>
            <p className="mt-2 text-sm text-[var(--text-secondary)]">
              Initialize the repository with a guided configuration flow. You can confirm the detected project root or override it before writing `.macc/macc.yaml`.
            </p>

            <label className="mt-5 flex flex-col gap-2">
              <span className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)]">
                Project root
              </span>
              <input
                aria-label="Project root"
                className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] px-4 py-3 text-sm text-[var(--text-primary)] outline-none ring-0 transition focus:border-[var(--accent)]"
                onChange={(event) => {
                  setProjectRoot(event.target.value);
                  setStepError(null);
                }}
                placeholder={detectedRoot || '.'}
                value={projectRoot}
              />
            </label>

            <div className="mt-4 rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4 text-sm text-[var(--text-secondary)]">
              <p className="font-medium text-[var(--text-primary)]">Detected from backend</p>
              <p className="mt-1 font-mono text-xs break-all">{detectedRoot || '.'}</p>
            </div>
          </section>

          <aside className="rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] p-5">
            <h3 className="text-sm font-semibold uppercase tracking-wide text-[var(--text-muted)]">What happens next</h3>
            <ul className="mt-3 space-y-3 text-sm text-[var(--text-secondary)]">
              <li>Tool detection previews enabled adapters, versions, and health.</li>
              <li>Standards presets generate the initial `macc.yaml` content.</li>
              <li>Review shows the final YAML before saving.</li>
            </ul>
          </aside>
        </div>
      );
    }

    if (step === 1) {
      return (
        <div className="grid gap-4">
          {toolOptions.map((tool) => (
            <label
              key={tool.id}
              className="flex items-start justify-between gap-4 rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] p-4 transition hover:border-white/20 hover:bg-white/5"
            >
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-semibold text-[var(--text-primary)]">{titleCase(tool.id)}</span>
                  <StatusBadge status={tool.health} tone={tool.health === 'healthy' ? 'merged' : 'blocked'} />
                </div>
                <p className="mt-1 text-sm text-[var(--text-secondary)]">
                  Version: <span className="font-mono">{tool.version}</span>
                </p>
              </div>
              <input
                aria-label={`Enable ${tool.id}`}
                checked={selectedTools.has(tool.id)}
                className="mt-1 h-4 w-4 accent-[var(--accent)]"
                onChange={() => toggleTool(tool.id)}
                type="checkbox"
              />
            </label>
          ))}

          {toolOptions.length === 0 && (
            <div className="rounded-2xl border border-dashed border-[var(--border)] bg-[var(--bg-card)] p-6 text-sm text-[var(--text-secondary)]">
              No tools were detected yet. Finish setup with the defaults or return after tools are available.
            </div>
          )}
        </div>
      );
    }

    if (step === 2) {
      return (
        <div className="grid gap-6 lg:grid-cols-[0.8fr_1.2fr]">
          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] p-5">
            <h2 className="text-lg font-semibold text-[var(--text-primary)]">Standards preset</h2>
            <p className="mt-2 text-sm text-[var(--text-secondary)]">
              Choose the initial standards profile for generated docs and repo guidance.
            </p>
            <div className="mt-4 grid gap-3">
              {PRESETS.map((preset) => (
                <label
                  key={preset.id}
                  className={cn(
                    'rounded-2xl border p-4 transition',
                    preset.id === presetId
                      ? 'border-[var(--accent)] bg-[var(--accent)]/10'
                      : 'border-[var(--border)] bg-[var(--bg-secondary)] hover:border-white/20',
                  )}
                >
                  <div className="flex items-start gap-3">
                    <input
                      checked={preset.id === presetId}
                      className="mt-1 h-4 w-4 accent-[var(--accent)]"
                      onChange={() => {
                        setPresetId(preset.id);
                        setStepError(null);
                      }}
                      name="standards-preset"
                      type="radio"
                      value={preset.id}
                    />
                    <div>
                      <div className="font-medium text-[var(--text-primary)]">{preset.label}</div>
                      <p className="mt-1 text-sm text-[var(--text-secondary)]">{preset.description}</p>
                    </div>
                  </div>
                </label>
              ))}
            </div>
          </section>

          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] p-5">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <h3 className="text-lg font-semibold text-[var(--text-primary)]">Rendered preview</h3>
                <p className="text-sm text-[var(--text-secondary)]">
                  The backend preview shows what the generated standards files will contain.
                </p>
              </div>
              {isPreviewLoading && <LoadingSpinner label="Refreshing preview" />}
            </div>

            {previewError && (
              <div className="mt-4 rounded-xl border border-rose-500/30 bg-rose-500/10 p-4 text-sm text-rose-200">
                Preview refresh failed: {previewError}
              </div>
            )}

            <div className="mt-4 grid gap-4">
              {previewCards.length > 0 ? (
                previewCards.map((card) => (
                  <article key={card.id} className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4">
                    <h4 className="text-sm font-semibold text-[var(--text-primary)]">{card.title}</h4>
                    <pre className="mt-3 overflow-x-auto whitespace-pre-wrap break-words font-mono text-xs leading-6 text-[var(--text-secondary)]">
                      {card.content}
                    </pre>
                  </article>
                ))
              ) : (
                <div className="rounded-2xl border border-dashed border-[var(--border)] bg-[var(--bg-secondary)] p-4 text-sm text-[var(--text-secondary)]">
                  No preview returned yet.
                </div>
              )}
            </div>
          </section>
        </div>
      );
    }

    return (
      <div className="grid gap-6 lg:grid-cols-[1fr_0.95fr]">
        <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] p-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h2 className="text-lg font-semibold text-[var(--text-primary)]">Config preview</h2>
              <p className="mt-2 text-sm text-[var(--text-secondary)]">
                Review the YAML that will be written before saving the initialization files.
              </p>
            </div>
            <Button disabled={isPlanPreviewLoading} onClick={handlePlanPreview} type="button">
              {isPlanPreviewLoading ? 'Previewing plan...' : 'Preview plan'}
            </Button>
          </div>

          <pre className="mt-4 max-h-[30rem] overflow-auto rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4 font-mono text-xs leading-6 text-[var(--text-secondary)]">
            {previewConfig ? toYaml(previewConfig) : 'Loading preview...'}
          </pre>
        </section>

        <aside className="grid gap-4">
          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] p-5">
            <h3 className="text-lg font-semibold text-[var(--text-primary)]">Initialization summary</h3>
            <dl className="mt-4 grid gap-3 text-sm">
              <div className="flex items-center justify-between gap-4">
                <dt className="text-[var(--text-secondary)]">Project root</dt>
                <dd className="font-mono text-[var(--text-primary)] break-all">{projectRoot || detectedRoot || '.'}</dd>
              </div>
              <div className="flex items-center justify-between gap-4">
                <dt className="text-[var(--text-secondary)]">Enabled tools</dt>
                <dd className="text-[var(--text-primary)]">{selectedTools.size}</dd>
              </div>
              <div className="flex items-center justify-between gap-4">
                <dt className="text-[var(--text-secondary)]">Standards preset</dt>
                <dd className="text-[var(--text-primary)]">{PRESETS.find((preset) => preset.id === presetId)?.label ?? presetId}</dd>
              </div>
            </dl>
          </section>

          <section className="rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] p-5">
            <div className="flex items-center justify-between gap-3">
              <h3 className="text-lg font-semibold text-[var(--text-primary)]">Optional plan preview</h3>
              {isPlanPreviewLoading && <LoadingSpinner label="Loading plan preview" />}
            </div>
            <p className="mt-2 text-sm text-[var(--text-secondary)]">
              Generate a draft plan for the initialized configuration without applying changes.
            </p>
            {planPreviewError && (
              <div className="mt-4 rounded-xl border border-rose-500/30 bg-rose-500/10 p-4 text-sm text-rose-200">
                {planPreviewError}
              </div>
            )}
            {planPreview && (
              <div className="mt-4 grid gap-3 text-sm text-[var(--text-secondary)]">
                <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4">
                  <p className="font-medium text-[var(--text-primary)]">Summary</p>
                  <p className="mt-1">Total actions: {planPreview.summary.totalActions}</p>
                  <p>Files to write: {planPreview.summary.filesWrite}</p>
                  <p>Files to merge: {planPreview.summary.filesMerge}</p>
                </div>
                <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4">
                  <p className="font-medium text-[var(--text-primary)]">Risks</p>
                  {planPreview.risks.length === 0 ? (
                    <p className="mt-1">No risks returned.</p>
                  ) : (
                    <ul className="mt-2 list-disc space-y-1 pl-5">
                      {planPreview.risks.map((risk) => (
                        <li key={`${risk.level}-${risk.message}`}>{risk.message}</li>
                      ))}
                    </ul>
                  )}
                </div>
              </div>
            )}
          </section>
        </aside>
      </div>
    );
  };

  if (isLoading) {
    return (
      <div className="flex min-h-[48vh] items-center justify-center">
        <LoadingSpinner label="Loading initialization wizard" />
      </div>
    );
  }

  if (loadError) {
    return (
      <div className="flex flex-col gap-6 pb-8">
        <header className="rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6 shadow-sm">
          <h1 className="text-3xl font-semibold tracking-tight text-[var(--text-primary)]">Project initialization</h1>
          <p className="mt-2 max-w-3xl text-sm text-[var(--text-secondary)]">
            Guided repository setup failed to load.
          </p>
        </header>
        <ErrorBanner title="Unable to load initialization wizard" message={loadError} />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6 pb-8">
      <header className="rounded-3xl border border-[var(--border)] bg-[linear-gradient(135deg,rgba(59,130,246,0.12),rgba(255,255,255,0.02))] p-6 shadow-sm">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="max-w-3xl">
            <p className="text-xs font-semibold uppercase tracking-[0.3em] text-[var(--text-muted)]">macc init --wizard</p>
            <h1 className="mt-3 text-4xl font-semibold tracking-tight text-[var(--text-primary)]">
              Project initialization wizard
            </h1>
            <p className="mt-3 text-sm leading-6 text-[var(--text-secondary)]">
              Walk through root selection, tool detection, standards generation, and final review before writing `.macc/macc.yaml`.
            </p>
          </div>
          <div className="rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] px-4 py-3 text-right">
            <div className="text-xs uppercase tracking-[0.24em] text-[var(--text-muted)]">Step</div>
            <div className="mt-1 text-2xl font-semibold text-[var(--text-primary)]">{stepIndexLabel}</div>
          </div>
        </div>

        <div className="mt-5">
          <div className="flex items-center justify-between text-xs uppercase tracking-wide text-[var(--text-muted)]">
            <span>Progress</span>
            <span>{STEPS[step]}</span>
          </div>
          <div className="mt-3 h-2 overflow-hidden rounded-full bg-white/5">
            <div
              className="h-full rounded-full bg-[var(--accent)] transition-[width] duration-300"
              style={{ width: `${progressPercent}%` }}
            />
          </div>
          <ol className="mt-4 grid gap-2 md:grid-cols-4" aria-label="Initialization progress">
            {STEPS.map((label, index) => {
              const isActive = index === step;
              const isDone = index < step;
              return (
                <li
                  key={label}
                  className={cn(
                    'rounded-2xl border px-4 py-3 text-sm transition',
                    isActive
                      ? 'border-[var(--accent)] bg-[var(--accent)]/10 text-[var(--text-primary)]'
                      : isDone
                        ? 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200'
                        : 'border-[var(--border)] bg-[var(--bg-card)] text-[var(--text-secondary)]',
                  )}
                >
                  <span className="block text-[10px] uppercase tracking-[0.24em]">Step {index + 1}</span>
                  <span className="mt-1 block font-medium">{label}</span>
                </li>
              );
            })}
          </ol>
        </div>
      </header>

      {stepError && <ErrorBanner title="Validation required" message={stepError} />}
      {saveError && <ErrorBanner title="Initialization failed" message={saveError} />}

      <section className="rounded-3xl border border-[var(--border)] bg-[var(--bg-secondary)] p-5 shadow-sm">
        {renderStepContent()}
      </section>

      <footer className="flex flex-wrap items-center justify-between gap-3 rounded-3xl border border-[var(--border)] bg-[var(--bg-card)] px-5 py-4">
        <Button disabled={step === 0 || isSaving} onClick={goBack} type="button">
          Back
        </Button>
        <div className="flex flex-wrap items-center gap-3">
          <Button onClick={useDefaults} type="button">
            Skip to defaults
          </Button>
          {step < 3 ? (
            <Button onClick={goNext} type="button">
              Next
            </Button>
          ) : (
            <label className="flex items-center gap-2 rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-2 text-sm text-[var(--text-secondary)]">
              <input
                checked={reviewAccepted}
                className="h-4 w-4 accent-[var(--accent)]"
                onChange={(event) => {
                  setReviewAccepted(event.target.checked);
                  setStepError(null);
                }}
                type="checkbox"
              />
              I reviewed the config preview
            </label>
          )}
          {step === 3 ? (
            <Button disabled={isSaving} onClick={handleFinish} type="button">
              {isSaving ? 'Creating project...' : 'Create project'}
            </Button>
          ) : null}
        </div>
      </footer>
    </div>
  );
};

export default Init;
