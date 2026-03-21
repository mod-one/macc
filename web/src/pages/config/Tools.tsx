import React from 'react';
import { Link } from 'react-router-dom';
import { getConfig, updateConfig, ApiClientError } from '../../api/client';
import type { ApiConfigResponse, JsonValue } from '../../api/models';
import { Button, RightDrawer, StatusBadge } from '../../components';
import { CopyIcon, RefreshIcon, SearchIcon } from '../../components/icons';
import { cn } from '../../components/styles';

type ToolFilter = 'all' | 'enabled' | 'installed';
type ToolHealth = 'healthy' | 'degraded';
type ToolActivity = 'idle' | 'active';
type JsonObject = Record<string, JsonValue>;
type FieldKind = 'string' | 'number' | 'boolean' | 'json';

interface ToolViewModel {
  id: string;
  name: string;
  version: string;
  category: string;
  capabilities: string[];
  enabled: boolean;
  installed: boolean;
  health: ToolHealth;
  activity: ToolActivity;
}

interface SchemaField {
  path: string;
  label: string;
  kind: FieldKind;
  placeholder?: string;
}

interface SchemaSection {
  id: string;
  title: string;
  fields: SchemaField[];
}

const SCHEMA_SECTIONS: SchemaSection[] = [
  {
    id: 'container',
    title: 'Container Settings',
    fields: [
      { path: 'container.image', label: 'Image', kind: 'string', placeholder: 'ghcr.io/org/tool:latest' },
      { path: 'container.command', label: 'Command', kind: 'string', placeholder: 'tool-server' },
      { path: 'container.args', label: 'Arguments', kind: 'json', placeholder: '["--stdio"]' },
      { path: 'container.workingDir', label: 'Working Directory', kind: 'string', placeholder: '/workspace' },
    ],
  },
  {
    id: 'mounts',
    title: 'Mounts',
    fields: [
      { path: 'mounts', label: 'Mount List', kind: 'json', placeholder: '[{"source":".","target":"/workspace"}]' },
    ],
  },
  {
    id: 'network',
    title: 'Network Access',
    fields: [
      { path: 'network.enabled', label: 'Network Enabled', kind: 'boolean' },
      { path: 'network.allowedHosts', label: 'Allowed Hosts', kind: 'json', placeholder: '["api.openai.com"]' },
      { path: 'network.mode', label: 'Mode', kind: 'string', placeholder: 'restricted' },
    ],
  },
  {
    id: 'env',
    title: 'Environment',
    fields: [
      { path: 'env', label: 'Environment Variables', kind: 'json', placeholder: '{"LOG_LEVEL":"debug"}' },
    ],
  },
  {
    id: 'timeouts',
    title: 'Timeouts',
    fields: [
      { path: 'timeouts.startupSeconds', label: 'Startup Timeout (s)', kind: 'number' },
      { path: 'timeouts.execSeconds', label: 'Execution Timeout (s)', kind: 'number' },
      { path: 'timeouts.idleSeconds', label: 'Idle Timeout (s)', kind: 'number' },
    ],
  },
];

function isJsonObject(value: JsonValue | undefined): value is JsonObject {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function cloneJson<T extends JsonValue>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function titleCaseToolId(value: string): string {
  return value
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
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

function asStringArray(value: JsonValue | undefined): string[] {
  if (Array.isArray(value)) {
    return value
      .filter((entry): entry is string => typeof entry === 'string')
      .map((entry) => entry.trim())
      .filter(Boolean);
  }
  if (typeof value === 'string' && value.trim().length > 0) {
    return value
      .split(',')
      .map((part) => part.trim())
      .filter(Boolean);
  }
  return [];
}

function getNestedValue(source: JsonObject, path: string): JsonValue | undefined {
  const segments = path.split('.').filter(Boolean);
  let current: JsonValue = source;

  for (const segment of segments) {
    if (!isJsonObject(current)) {
      return undefined;
    }
    current = current[segment];
    if (typeof current === 'undefined') {
      return undefined;
    }
  }

  return current;
}

function setNestedValue(source: JsonObject, path: string, value: JsonValue): JsonObject {
  const segments = path.split('.').filter(Boolean);
  if (segments.length === 0) {
    return source;
  }

  const draft = cloneJson(source);
  let cursor: JsonObject = draft;

  for (let index = 0; index < segments.length - 1; index += 1) {
    const segment = segments[index];
    const next = cursor[segment];
    if (!isJsonObject(next)) {
      cursor[segment] = {};
    }
    cursor = cursor[segment] as JsonObject;
  }

  cursor[segments[segments.length - 1]] = value;
  return draft;
}

function normalizeToolIds(config: ApiConfigResponse): string[] {
  const ids = new Set<string>();
  for (const id of config.enabledTools) {
    ids.add(id);
  }
  for (const id of Object.keys(config.toolSettings)) {
    ids.add(id);
  }
  for (const id of Object.keys(config.toolConfig)) {
    ids.add(id);
  }
  for (const id of config.toolPriority) {
    ids.add(id);
  }
  return Array.from(ids).sort((a, b) => a.localeCompare(b));
}

function buildToolViewModel(
  toolId: string,
  enabledSet: Set<string>,
  toolConfig: Record<string, JsonValue>,
  toolSettings: Record<string, JsonValue>,
): ToolViewModel {
  const config = isJsonObject(toolConfig[toolId]) ? (toolConfig[toolId] as JsonObject) : {};
  const settings = isJsonObject(toolSettings[toolId]) ? (toolSettings[toolId] as JsonObject) : {};

  const version =
    asString(config.version) ??
    asString(settings.version) ??
    asString(config.adapterVersion) ??
    asString(settings.adapterVersion) ??
    'n/a';

  const category =
    asString(config.category) ??
    asString(settings.category) ??
    asString(config.type) ??
    asString(settings.type) ??
    'adapter';

  const capabilities = Array.from(
    new Set([
      ...asStringArray(config.capabilities),
      ...asStringArray(settings.capabilities),
      ...asStringArray(config.features),
      ...asStringArray(settings.features),
    ]),
  );

  const healthyValue = asBoolean(settings.healthy) ?? asBoolean(config.healthy);
  const healthStatus =
    asString(settings.health) ??
    asString(config.health) ??
    asString(settings.status) ??
    asString(config.status) ??
    '';
  const health: ToolHealth =
    healthyValue === false || /(degraded|error|failed|unhealthy)/i.test(healthStatus)
      ? 'degraded'
      : 'healthy';

  const activeValue = asBoolean(settings.active) ?? asBoolean(config.active);
  const activityStatus = asString(settings.activity) ?? asString(settings.state) ?? asString(config.state) ?? '';
  const activity: ToolActivity =
    activeValue === true || /(active|running|busy)/i.test(activityStatus)
      ? 'active'
      : 'idle';

  const installed =
    (isJsonObject(config) && Object.keys(config).length > 0) ||
    (isJsonObject(settings) && Object.keys(settings).length > 0) ||
    enabledSet.has(toolId);

  return {
    id: toolId,
    name: titleCaseToolId(toolId),
    version,
    category,
    capabilities,
    enabled: enabledSet.has(toolId),
    installed,
    health,
    activity,
  };
}

function formatApiError(error: unknown): string {
  if (error instanceof ApiClientError) {
    return `${error.envelope.error.code}: ${error.envelope.error.message}`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return 'Unexpected tools configuration error.';
}

function jsonEquals(left: JsonValue, right: JsonValue): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

const Tools: React.FC = () => {
  const [config, setConfig] = React.useState<ApiConfigResponse | null>(null);
  const [draftToolSettings, setDraftToolSettings] = React.useState<Record<string, JsonValue>>({});
  const [draftEnabledTools, setDraftEnabledTools] = React.useState<Set<string>>(new Set());
  const [isLoading, setIsLoading] = React.useState(true);
  const [isRefreshing, setIsRefreshing] = React.useState(false);
  const [isSaving, setIsSaving] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const [searchTerm, setSearchTerm] = React.useState('');
  const [filter, setFilter] = React.useState<ToolFilter>('all');

  const [selectedToolId, setSelectedToolId] = React.useState<string | null>(null);
  const [drawerOpen, setDrawerOpen] = React.useState(false);
  const [rawView, setRawView] = React.useState(false);
  const [rawEditorText, setRawEditorText] = React.useState('');
  const [rawEditorError, setRawEditorError] = React.useState<string | null>(null);
  const [copyState, setCopyState] = React.useState<'idle' | 'copied' | 'failed'>('idle');
  const [jsonFieldErrors, setJsonFieldErrors] = React.useState<Record<string, string>>({});
  const [jsonFieldDrafts, setJsonFieldDrafts] = React.useState<Record<string, string>>({});

  const loadConfig = React.useCallback(async (silent = false): Promise<void> => {
    if (silent) {
      setIsRefreshing(true);
    } else {
      setIsLoading(true);
      setError(null);
    }

    try {
      const nextConfig = await getConfig();
      setConfig(nextConfig);
      setDraftToolSettings(cloneJson(nextConfig.toolSettings));
      setDraftEnabledTools(new Set(nextConfig.enabledTools));
      setJsonFieldErrors({});
      setJsonFieldDrafts({});
      setRawEditorError(null);
      setError(null);
    } catch (loadError) {
      setError(formatApiError(loadError));
    } finally {
      setIsLoading(false);
      setIsRefreshing(false);
    }
  }, []);

  React.useEffect(() => {
    void loadConfig(false);
  }, [loadConfig]);

  const toolIds = React.useMemo(() => {
    if (!config) {
      return [];
    }
    return normalizeToolIds(config);
  }, [config]);

  const tools = React.useMemo(() => {
    if (!config) {
      return [];
    }
    return toolIds.map((id) => buildToolViewModel(id, draftEnabledTools, config.toolConfig, draftToolSettings));
  }, [config, toolIds, draftEnabledTools, draftToolSettings]);

  const filteredTools = React.useMemo(() => {
    const loweredSearch = searchTerm.trim().toLowerCase();

    return tools.filter((tool) => {
      if (filter === 'enabled' && !tool.enabled) {
        return false;
      }
      if (filter === 'installed' && !tool.installed) {
        return false;
      }
      if (loweredSearch.length === 0) {
        return true;
      }

      const haystack = [
        tool.id,
        tool.name,
        tool.version,
        tool.category,
        ...tool.capabilities,
      ]
        .join(' ')
        .toLowerCase();

      return haystack.includes(loweredSearch);
    });
  }, [tools, filter, searchTerm]);

  const selectedTool = React.useMemo(
    () => tools.find((tool) => tool.id === selectedToolId) ?? null,
    [tools, selectedToolId],
  );

  const selectedSettings = React.useMemo<JsonObject>(() => {
    if (!selectedToolId) {
      return {};
    }
    const raw = draftToolSettings[selectedToolId];
    return isJsonObject(raw) ? raw : {};
  }, [selectedToolId, draftToolSettings]);

  const selectedSettingsRaw = React.useMemo(
    () => JSON.stringify(selectedSettings, null, 2),
    [selectedSettings],
  );

  const hasSelectedUnsavedChanges = React.useMemo(() => {
    if (!config || !selectedToolId) {
      return false;
    }

    const savedSettings = config.toolSettings[selectedToolId] ?? {};
    const currentSettings = draftToolSettings[selectedToolId] ?? {};
    const enabledChanged =
      config.enabledTools.includes(selectedToolId) !== draftEnabledTools.has(selectedToolId);

    return !jsonEquals(savedSettings, currentSettings) || enabledChanged;
  }, [config, selectedToolId, draftEnabledTools, draftToolSettings]);

  const hasAnyUnsavedChanges = React.useMemo(() => {
    if (!config) {
      return false;
    }

    if (config.enabledTools.length !== draftEnabledTools.size) {
      return true;
    }

    for (const enabledTool of config.enabledTools) {
      if (!draftEnabledTools.has(enabledTool)) {
        return true;
      }
    }

    const allToolIds = new Set<string>([
      ...Object.keys(config.toolSettings),
      ...Object.keys(draftToolSettings),
    ]);

    for (const toolId of allToolIds) {
      const saved = config.toolSettings[toolId] ?? {};
      const draft = draftToolSettings[toolId] ?? {};
      if (!jsonEquals(saved, draft)) {
        return true;
      }
    }

    return false;
  }, [config, draftEnabledTools, draftToolSettings]);

  const handleOpenTool = React.useCallback(
    (toolId: string) => {
      const initialSettings = isJsonObject(draftToolSettings[toolId]) ? (draftToolSettings[toolId] as JsonObject) : {};
      setSelectedToolId(toolId);
      setRawView(false);
      setRawEditorText(JSON.stringify(initialSettings, null, 2));
      setRawEditorError(null);
      setJsonFieldErrors({});
      setJsonFieldDrafts({});
      setDrawerOpen(true);
    },
    [draftToolSettings],
  );

  const handleDrawerOpenChange = React.useCallback(
    (nextOpen: boolean) => {
      if (!nextOpen && hasSelectedUnsavedChanges) {
        const proceed = window.confirm('You have unsaved changes for this adapter. Close without applying?');
        if (!proceed) {
          return;
        }
      }
      setDrawerOpen(nextOpen);
    },
    [hasSelectedUnsavedChanges],
  );

  const handleToggleEnabled = React.useCallback((toolId: string) => {
    setDraftEnabledTools((previous) => {
      const next = new Set(previous);
      if (next.has(toolId)) {
        next.delete(toolId);
      } else {
        next.add(toolId);
      }
      return next;
    });
  }, []);

  const handleFieldChange = React.useCallback(
    (path: string, value: JsonValue) => {
      if (!selectedToolId) {
        return;
      }

      setDraftToolSettings((previous) => {
        const toolSettings = isJsonObject(previous[selectedToolId]) ? (previous[selectedToolId] as JsonObject) : {};
        return {
          ...previous,
          [selectedToolId]: setNestedValue(toolSettings, path, value),
        };
      });
    },
    [selectedToolId],
  );

  const handleRevertSelected = React.useCallback(() => {
    if (!config || !selectedToolId) {
      return;
    }

    setDraftToolSettings((previous) => ({
      ...previous,
      [selectedToolId]: cloneJson(config.toolSettings[selectedToolId] ?? {}),
    }));

    setDraftEnabledTools((previous) => {
      const next = new Set(previous);
      if (config.enabledTools.includes(selectedToolId)) {
        next.add(selectedToolId);
      } else {
        next.delete(selectedToolId);
      }
      return next;
    });

    setJsonFieldErrors({});
    setJsonFieldDrafts({});
    setRawEditorText(JSON.stringify(config.toolSettings[selectedToolId] ?? {}, null, 2));
    setRawEditorError(null);
  }, [config, selectedToolId]);

  const handleApplyChanges = React.useCallback(async (): Promise<void> => {
    if (!config || !selectedToolId) {
      return;
    }

    setIsSaving(true);
    setError(null);

    try {
      const updated = await updateConfig({
        enabledTools: Array.from(draftEnabledTools).sort((a, b) => a.localeCompare(b)),
        toolSettings: cloneJson(draftToolSettings),
      });

      setConfig(updated);
      setDraftToolSettings(cloneJson(updated.toolSettings));
      setDraftEnabledTools(new Set(updated.enabledTools));
      setJsonFieldErrors({});
      setJsonFieldDrafts({});
      setRawEditorError(null);
    } catch (saveError) {
      setError(formatApiError(saveError));
    } finally {
      setIsSaving(false);
    }
  }, [config, selectedToolId, draftEnabledTools, draftToolSettings]);

  const handleRawCopy = React.useCallback(async (): Promise<void> => {
    try {
      await navigator.clipboard.writeText(selectedSettingsRaw);
      setCopyState('copied');
      window.setTimeout(() => setCopyState('idle'), 1500);
    } catch {
      setCopyState('failed');
      window.setTimeout(() => setCopyState('idle'), 1500);
    }
  }, [selectedSettingsRaw]);

  const renderField = React.useCallback(
    (field: SchemaField) => {
      if (!selectedToolId) {
        return null;
      }

      const fieldKey = `${selectedToolId}:${field.path}`;
      const value = getNestedValue(selectedSettings, field.path);
      const jsonError = jsonFieldErrors[fieldKey];

      if (field.kind === 'boolean') {
        const selectedValue = typeof value === 'boolean' ? String(value) : 'unset';
        return (
          <div key={field.path} className="space-y-1.5">
            <label className="text-xs font-semibold text-[var(--text-secondary)]">{field.label}</label>
            <select
              className="h-10 w-full rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40"
              value={selectedValue}
              onChange={(event) => {
                const raw = event.target.value;
                if (raw === 'true') {
                  handleFieldChange(field.path, true);
                } else if (raw === 'false') {
                  handleFieldChange(field.path, false);
                } else {
                  handleFieldChange(field.path, null);
                }
              }}
            >
              <option value="unset">Unset</option>
              <option value="true">True</option>
              <option value="false">False</option>
            </select>
          </div>
        );
      }

      if (field.kind === 'number') {
        const numberValue = typeof value === 'number' ? String(value) : '';
        return (
          <div key={field.path} className="space-y-1.5">
            <label className="text-xs font-semibold text-[var(--text-secondary)]">{field.label}</label>
            <input
              className="h-10 w-full rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40"
              placeholder={field.placeholder}
              type="number"
              value={numberValue}
              onChange={(event) => {
                const next = event.target.value;
                if (next.trim() === '') {
                  handleFieldChange(field.path, null);
                  return;
                }
                const parsed = Number(next);
                if (!Number.isNaN(parsed)) {
                  handleFieldChange(field.path, parsed);
                }
              }}
            />
          </div>
        );
      }

      if (field.kind === 'json') {
        const draftValue = jsonFieldDrafts[fieldKey] ?? JSON.stringify(value ?? null, null, 2);
        return (
          <div key={field.path} className="space-y-1.5">
            <label className="text-xs font-semibold text-[var(--text-secondary)]">{field.label}</label>
            <textarea
              className={cn(
                'min-h-[92px] w-full rounded-lg border bg-[var(--bg-secondary)] px-3 py-2 font-mono text-xs text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40',
                jsonError ? 'border-[var(--error)]' : 'border-[var(--border)]',
              )}
              placeholder={field.placeholder}
              value={draftValue}
              onChange={(event) => {
                const nextText = event.target.value;
                setJsonFieldDrafts((previous) => ({
                  ...previous,
                  [fieldKey]: nextText,
                }));
              }}
              onBlur={(event) => {
                const nextText = event.target.value.trim();
                if (nextText.length === 0) {
                  handleFieldChange(field.path, null);
                  setJsonFieldErrors((previous) => {
                    const next = { ...previous };
                    delete next[fieldKey];
                    return next;
                  });
                  return;
                }

                try {
                  const parsed = JSON.parse(nextText) as JsonValue;
                  handleFieldChange(field.path, parsed);
                  setJsonFieldErrors((previous) => {
                    const next = { ...previous };
                    delete next[fieldKey];
                    return next;
                  });
                } catch {
                  setJsonFieldErrors((previous) => ({
                    ...previous,
                    [fieldKey]: 'Invalid JSON. Fix before applying.',
                  }));
                }
              }}
            />
            {jsonError && <p className="text-xs text-[var(--error)]">{jsonError}</p>}
          </div>
        );
      }

      const stringValue = typeof value === 'string' ? value : '';
      return (
        <div key={field.path} className="space-y-1.5">
          <label className="text-xs font-semibold text-[var(--text-secondary)]">{field.label}</label>
          <input
            className="h-10 w-full rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40"
            placeholder={field.placeholder}
            value={stringValue}
            onChange={(event) => handleFieldChange(field.path, event.target.value)}
          />
        </div>
      );
    },
    [selectedToolId, selectedSettings, jsonFieldErrors, jsonFieldDrafts, handleFieldChange],
  );

  const hasBlockingEditorErrors = Object.keys(jsonFieldErrors).length > 0 || (rawView && rawEditorError !== null);

  if (isLoading) {
    return (
      <div className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-6 text-[var(--text-secondary)] shadow-[var(--shadow-soft)]">
        Loading tools configuration...
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      <header className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[radial-gradient(circle_at_top_left,_rgba(59,130,246,0.18),_transparent_35%),var(--bg-secondary)] p-6 shadow-[var(--shadow-soft)]">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="space-y-2">
            <h1 className="text-3xl font-semibold tracking-tight text-[var(--text-primary)]">Tools & Adapters</h1>
            <p className="max-w-3xl text-sm text-[var(--text-secondary)]">
              Configure adapter settings, runtime state, capabilities, and health from one place.
            </p>
          </div>
          <div className="flex items-center gap-2">
            <Button
              className="border-[var(--border)] bg-[var(--bg-card)]"
              disabled={isRefreshing}
              onClick={() => {
                if (hasAnyUnsavedChanges) {
                  const proceed = window.confirm(
                    'You have unsaved adapter changes. Refreshing will discard them. Continue?',
                  );
                  if (!proceed) {
                    return;
                  }
                }
                void loadConfig(true);
              }}
              type="button"
            >
              <RefreshIcon className={cn('mr-2 h-4 w-4', isRefreshing && 'animate-spin')} />
              Check Updates
            </Button>
          </div>
        </div>

        <div className="mt-5 flex flex-wrap items-center gap-3">
          <div className="relative min-w-[230px] flex-1">
            <SearchIcon className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--text-muted)]" />
            <input
              className="h-10 w-full rounded-lg border border-[var(--border)] bg-[var(--bg-card)] pl-10 pr-3 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40"
              placeholder="Search adapters, capabilities, category"
              value={searchTerm}
              onChange={(event) => setSearchTerm(event.target.value)}
            />
          </div>

          <div className="inline-flex rounded-lg border border-[var(--border)] bg-[var(--bg-card)] p-1">
            {(['all', 'enabled', 'installed'] as ToolFilter[]).map((value) => (
              <button
                key={value}
                className={cn(
                  'rounded-md px-3 py-1.5 text-sm capitalize transition-colors',
                  filter === value
                    ? 'bg-[var(--accent)] text-white'
                    : 'text-[var(--text-secondary)] hover:bg-white/10 hover:text-[var(--text-primary)]',
                )}
                onClick={() => setFilter(value)}
                type="button"
              >
                {value}
              </button>
            ))}
          </div>
        </div>

        {hasAnyUnsavedChanges && (
          <div className="mt-4 rounded-lg border border-[var(--status-blocked)]/50 bg-[var(--status-blocked)]/10 px-3 py-2 text-xs text-[var(--text-primary)]">
            You have unsaved adapter changes. Open a card and apply from the editor drawer.
          </div>
        )}

        {error && (
          <div className="mt-4 rounded-lg border border-[var(--error)]/40 bg-[var(--error)]/10 px-3 py-2 text-sm text-[var(--text-primary)]">
            {error}
          </div>
        )}
      </header>

      {filteredTools.length === 0 ? (
        <section className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-8 text-center text-sm text-[var(--text-secondary)] shadow-[var(--shadow-soft)]">
          No tools matched the current filters.
        </section>
      ) : (
        <section className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {filteredTools.map((tool) => (
            <article
              key={tool.id}
              className="group flex cursor-pointer flex-col gap-4 rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-4 shadow-[var(--shadow-soft)] transition-colors hover:border-white/15 hover:bg-white/[0.03]"
              onClick={() => handleOpenTool(tool.id)}
            >
              <div className="flex items-start justify-between gap-3">
                <div>
                  <h2 className="text-lg font-semibold text-[var(--text-primary)]">{tool.name}</h2>
                  <p className="text-xs uppercase tracking-wide text-[var(--text-muted)]">
                    {tool.id} · v{tool.version}
                  </p>
                </div>
                <label
                  className="inline-flex items-center gap-2 rounded-full border border-[var(--border)] bg-[var(--bg-secondary)] px-2.5 py-1 text-xs"
                  onClick={(event) => event.stopPropagation()}
                >
                  <input
                    checked={tool.enabled}
                    className="h-4 w-4 rounded border-[var(--border)] bg-transparent text-[var(--accent)]"
                    onChange={() => handleToggleEnabled(tool.id)}
                    type="checkbox"
                  />
                  <span className={tool.enabled ? 'text-[var(--accent)]' : 'text-[var(--text-secondary)]'}>
                    {tool.enabled ? 'Enabled' : 'Disabled'}
                  </span>
                </label>
              </div>

              <div className="flex flex-wrap items-center gap-2">
                <StatusBadge
                  status={tool.health}
                  tone={tool.health === 'healthy' ? 'active' : 'blocked'}
                />
                <StatusBadge
                  status={tool.activity}
                  tone={tool.activity === 'active' ? 'active' : 'todo'}
                />
                <span className="inline-flex rounded-full border border-[var(--border)] bg-[var(--bg-secondary)] px-2.5 py-1 text-xs uppercase text-[var(--text-secondary)]">
                  {tool.category}
                </span>
              </div>

              <div className="flex flex-wrap gap-2">
                {tool.capabilities.length > 0 ? (
                  tool.capabilities.slice(0, 4).map((capability) => (
                    <span
                      key={`${tool.id}-${capability}`}
                      className="rounded-full border border-[var(--border)] bg-[var(--bg-secondary)] px-2 py-1 text-xs text-[var(--text-secondary)]"
                    >
                      {capability}
                    </span>
                  ))
                ) : (
                  <span className="text-xs text-[var(--text-muted)]">No capability metadata.</span>
                )}
              </div>

              {!tool.installed && (
                <div className="mt-auto rounded-lg border border-[var(--accent)]/30 bg-[var(--accent)]/10 p-2 text-xs text-[var(--text-secondary)]">
                  Adapter not installed.{' '}
                  <Link className="font-semibold text-[var(--accent)] hover:underline" to="/ops/console">
                    Open terminal setup
                  </Link>
                </div>
              )}
            </article>
          ))}
        </section>
      )}

      <RightDrawer
        description={selectedTool ? `${selectedTool.id} adapter configuration` : undefined}
        footer={
          selectedTool ? (
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="text-xs text-[var(--text-secondary)]">
                {hasSelectedUnsavedChanges ? 'Unsaved changes detected' : 'No pending edits'}
              </div>
              <div className="flex items-center gap-2">
                <Button
                  className="border-[var(--border)] bg-[var(--bg-card)]"
                  disabled={!hasSelectedUnsavedChanges || isSaving}
                  onClick={handleRevertSelected}
                  type="button"
                >
                  Revert
                </Button>
                <Button
                  className="border-transparent bg-[var(--accent)] text-white hover:brightness-110"
                  disabled={!hasAnyUnsavedChanges || isSaving || hasBlockingEditorErrors}
                  onClick={() => {
                    void handleApplyChanges();
                  }}
                  type="button"
                >
                  {isSaving ? 'Applying...' : 'Apply Changes'}
                </Button>
              </div>
            </div>
          ) : null
        }
        onOpenChange={handleDrawerOpenChange}
        open={drawerOpen}
        title={selectedTool ? `${selectedTool.name} Settings` : 'Tool Settings'}
        widthClassName="w-full max-w-2xl"
      >
        {!selectedTool ? (
          <p className="text-sm text-[var(--text-secondary)]">Select a tool card to inspect and edit settings.</p>
        ) : (
          <div className="space-y-5">
            <div className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-4">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div>
                  <p className="text-sm font-semibold text-[var(--text-primary)]">Adapter Runtime</p>
                  <p className="text-xs text-[var(--text-secondary)]">Toggle runtime availability and inspect raw schema values.</p>
                </div>
                <label className="inline-flex items-center gap-2 rounded-full border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-1.5 text-xs">
                  <input
                    checked={draftEnabledTools.has(selectedTool.id)}
                    className="h-4 w-4 rounded border-[var(--border)] bg-transparent text-[var(--accent)]"
                    onChange={() => handleToggleEnabled(selectedTool.id)}
                    type="checkbox"
                  />
                  <span className="text-[var(--text-primary)]">Enabled</span>
                </label>
              </div>

              <div className="mt-4 flex flex-wrap items-center gap-2">
                <button
                  className={cn(
                    'rounded-md px-3 py-1.5 text-xs font-medium transition-colors',
                    !rawView
                      ? 'bg-[var(--accent)] text-white'
                      : 'border border-[var(--border)] bg-[var(--bg-secondary)] text-[var(--text-secondary)]',
                  )}
                  onClick={() => setRawView(false)}
                  type="button"
                >
                  Form View
                </button>
                <button
                  className={cn(
                    'rounded-md px-3 py-1.5 text-xs font-medium transition-colors',
                    rawView
                      ? 'bg-[var(--accent)] text-white'
                      : 'border border-[var(--border)] bg-[var(--bg-secondary)] text-[var(--text-secondary)]',
                  )}
                  onClick={() => {
                    setRawEditorText(selectedSettingsRaw);
                    setRawEditorError(null);
                    setRawView(true);
                  }}
                  type="button"
                >
                  Raw JSON
                </button>
                <Button
                  className="h-8 border-[var(--border)] bg-[var(--bg-secondary)] px-2.5 text-xs"
                  onClick={() => {
                    void handleRawCopy();
                  }}
                  type="button"
                >
                  <CopyIcon className="mr-1.5 h-3.5 w-3.5" />
                  {copyState === 'copied' ? 'Copied' : copyState === 'failed' ? 'Copy Failed' : 'Copy'}
                </Button>
              </div>
            </div>

            {rawView ? (
              <>
                <textarea
                  aria-label="Raw JSON editor"
                  className={cn(
                    'min-h-[420px] w-full rounded-[var(--radius-card)] border bg-[var(--bg-card)] p-3 font-mono text-xs text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40',
                    rawEditorError ? 'border-[var(--error)]' : 'border-[var(--border)]',
                  )}
                  value={rawEditorText}
                  onChange={(event) => {
                    const nextRaw = event.target.value;
                    setRawEditorText(nextRaw);

                    const trimmed = nextRaw.trim();
                    if (trimmed.length === 0) {
                      setRawEditorError('Raw JSON must be a JSON object.');
                      return;
                    }

                    try {
                      const parsed = JSON.parse(nextRaw) as JsonValue;
                      if (!isJsonObject(parsed)) {
                        setRawEditorError('Raw JSON must be a JSON object.');
                        return;
                      }

                      setDraftToolSettings((previous) => ({
                        ...previous,
                        [selectedTool.id]: parsed,
                      }));
                      setRawEditorError(null);
                    } catch {
                      setRawEditorError('Invalid JSON. Fix before applying.');
                    }
                  }}
                />
                {rawEditorError && <p className="mt-2 text-xs text-[var(--error)]">{rawEditorError}</p>}
              </>
            ) : (
              <div className="space-y-4">
                {SCHEMA_SECTIONS.map((section) => (
                  <section
                    key={section.id}
                    className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-4"
                  >
                    <h3 className="mb-3 text-sm font-semibold text-[var(--text-primary)]">{section.title}</h3>
                    <div className="grid gap-3 sm:grid-cols-2">{section.fields.map((field) => renderField(field))}</div>
                  </section>
                ))}
              </div>
            )}
          </div>
        )}
      </RightDrawer>
    </div>
  );
};

export default Tools;
