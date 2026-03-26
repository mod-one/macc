import React from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { ApiClientError, getConfig, getToolDescriptors, updateConfig } from '../../api/client';
import type {
  ApiConfigResponse,
  ApiToolActionKind,
  ApiToolDescriptor,
  ApiToolField,
  ApiToolFieldDefault,
  JsonValue,
} from '../../api/models';
import { Button, RightDrawer, StatusBadge } from '../../components';
import { CopyIcon, RefreshIcon, SearchIcon } from '../../components/icons';
import { cn } from '../../components/styles';

type ToolFilter = 'all' | 'enabled' | 'installed';
type ToolHealth = 'healthy' | 'degraded';
type ToolActivity = 'idle' | 'active';
type JsonObject = Record<string, JsonValue>;

interface ToolViewModel {
  id: string;
  name: string;
  description: string;
  version: string;
  category: string;
  capabilities: string[];
  enabled: boolean;
  installed: boolean;
  health: ToolHealth;
  activity: ToolActivity;
}

interface ToolFieldSection {
  id: string;
  title: string;
  fields: ApiToolField[];
}

function isJsonObject(value: JsonValue | undefined): value is JsonObject {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function cloneJson<T extends JsonValue>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function titleCaseToolId(value: string): string {
  return value
    .split(/[-_\s/]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}

function titleCaseSection(value: string): string {
  return value
    .split(/[-_\s/]+/)
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
      .map((entry) => entry.trim())
      .filter(Boolean);
  }
  return [];
}

function jsonEquals(left: JsonValue, right: JsonValue): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function ensureJsonRecord(value: unknown): Record<string, JsonValue> {
  return isJsonObject(value as JsonValue | undefined) ? (value as Record<string, JsonValue>) : {};
}

function ensureStringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === 'string') : [];
}

function ensureToolFields(value: unknown): ApiToolField[] {
  return Array.isArray(value) ? value.filter((entry): entry is ApiToolField => Boolean(entry) && typeof entry === 'object') : [];
}

function normalizeConfigResponse(config: ApiConfigResponse): ApiConfigResponse {
  return {
    ...config,
    enabledTools: ensureStringArray(config.enabledTools),
    toolConfig: ensureJsonRecord(config.toolConfig),
    toolSettings: ensureJsonRecord(config.toolSettings),
    toolPriority: ensureStringArray(config.toolPriority),
  };
}

function normalizeToolDescriptors(value: unknown): ApiToolDescriptor[] {
  if (!Array.isArray(value)) {
    return [];
  }

  return value
    .filter((entry): entry is ApiToolDescriptor => Boolean(entry) && typeof entry === 'object')
    .map((descriptor) => ({
      id: typeof descriptor.id === 'string' ? descriptor.id : '',
      title:
        typeof descriptor.title === 'string' && descriptor.title.trim().length > 0
          ? descriptor.title
          : titleCaseToolId(typeof descriptor.id === 'string' ? descriptor.id : ''),
      description: typeof descriptor.description === 'string' ? descriptor.description : '',
      fields: ensureToolFields(descriptor.fields),
      install: descriptor.install ?? null,
    }))
    .filter((descriptor) => descriptor.id.trim().length > 0)
    .sort((left, right) => left.id.localeCompare(right.id));
}

function decodePointerSegment(segment: string): string {
  return segment.replace(/~1/g, '/').replace(/~0/g, '~');
}

function splitJsonPointer(pointer: string): string[] {
  if (pointer === '' || pointer === '/') {
    return [];
  }

  return pointer
    .split('/')
    .slice(1)
    .map(decodePointerSegment)
    .filter(Boolean);
}

function relativeToolPointer(toolId: string, pointer: string): string | null {
  const configPrefix = `/tools/config/${toolId}`;
  const legacyPrefix = `/tools/${toolId}`;

  if (pointer === configPrefix || pointer === legacyPrefix) {
    return '/';
  }
  if (pointer.startsWith(`${configPrefix}/`)) {
    return pointer.slice(configPrefix.length);
  }
  if (pointer.startsWith(`${legacyPrefix}/`)) {
    return pointer.slice(legacyPrefix.length);
  }
  return null;
}

function getValueAtPointer(source: JsonObject, pointer: string): JsonValue | undefined {
  const segments = splitJsonPointer(pointer);
  if (segments.length === 0) {
    return source;
  }

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

function removeEmptyObjects(value: JsonValue | undefined): JsonValue | undefined {
  if (!isJsonObject(value)) {
    return value;
  }

  const nextEntries = Object.entries(value)
    .map(([key, entry]) => [key, removeEmptyObjects(entry)] as const)
    .filter(([, entry]) => typeof entry !== 'undefined');

  if (nextEntries.length === 0) {
    return undefined;
  }

  return Object.fromEntries(nextEntries) as JsonObject;
}

function setValueAtPointer(source: JsonObject, pointer: string, value: JsonValue | undefined): JsonObject {
  const segments = splitJsonPointer(pointer);
  if (segments.length === 0) {
    return value && isJsonObject(value) ? cloneJson(value) : {};
  }

  const draft = cloneJson(source);
  let cursor: JsonObject = draft;
  const parents: Array<{ node: JsonObject; key: string }> = [];

  for (let index = 0; index < segments.length - 1; index += 1) {
    const segment = segments[index];
    const next = cursor[segment];
    if (!isJsonObject(next)) {
      cursor[segment] = {};
    }
    parents.push({ node: cursor, key: segment });
    cursor = cursor[segment] as JsonObject;
  }

  const finalKey = segments[segments.length - 1];
  if (typeof value === 'undefined') {
    delete cursor[finalKey];
    for (let index = parents.length - 1; index >= 0; index -= 1) {
      const { node, key } = parents[index];
      const cleaned = removeEmptyObjects(node[key]);
      if (typeof cleaned === 'undefined') {
        delete node[key];
      } else {
        node[key] = cleaned;
      }
    }
    return draft;
  }

  cursor[finalKey] = value;
  return draft;
}

function defaultValueForField(fieldDefault: ApiToolFieldDefault | null | undefined): JsonValue | undefined {
  if (!fieldDefault) {
    return undefined;
  }

  return fieldDefault.value as JsonValue;
}

function formatFieldDefault(fieldDefault: ApiToolFieldDefault | null | undefined): string | null {
  const value = defaultValueForField(fieldDefault);
  if (typeof value === 'undefined') {
    return null;
  }
  if (Array.isArray(value)) {
    return value.join(', ');
  }
  if (typeof value === 'object' && value !== null) {
    return JSON.stringify(value);
  }
  return String(value);
}

function normalizeToolIds(config: ApiConfigResponse, descriptors: ApiToolDescriptor[]): string[] {
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
  for (const descriptor of descriptors) {
    ids.add(descriptor.id);
  }
  return Array.from(ids).sort((a, b) => a.localeCompare(b));
}

function buildToolViewModel(
  toolId: string,
  enabledSet: Set<string>,
  toolConfig: Record<string, JsonValue>,
  toolSettings: Record<string, JsonValue>,
  descriptor: ApiToolDescriptor | null,
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
    name: descriptor?.title ?? titleCaseToolId(toolId),
    description: descriptor?.description ?? 'Tool adapter configuration.',
    version,
    category,
    capabilities,
    enabled: enabledSet.has(toolId),
    installed,
    health,
    activity,
  };
}

function buildFieldSections(toolId: string, descriptor: ApiToolDescriptor | null): ToolFieldSection[] {
  if (!descriptor) {
    return [];
  }

  const sectionMap = new Map<string, ToolFieldSection>();

  for (const field of descriptor.fields) {
    if (field.kind.type === 'action') {
      const current = sectionMap.get('actions');
      if (current) {
        current.fields.push(field);
      } else {
        sectionMap.set('actions', { id: 'actions', title: 'Connected Catalogs', fields: [field] });
      }
      continue;
    }

    const relativePointer = relativeToolPointer(toolId, field.path);
    const firstSegment = relativePointer ? splitJsonPointer(relativePointer)[0] ?? 'general' : 'general';
    const sectionId = firstSegment || 'general';
    const sectionTitle = sectionId === 'general' ? 'General' : titleCaseSection(sectionId);
    const current = sectionMap.get(sectionId);

    if (current) {
      current.fields.push(field);
    } else {
      sectionMap.set(sectionId, { id: sectionId, title: sectionTitle, fields: [field] });
    }
  }

  return Array.from(sectionMap.values());
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

function actionTarget(action: ApiToolActionKind): { to: string; label: string } | null {
  switch (action.action) {
    case 'openSkills':
      return { to: '/config/skills', label: 'Open Skills Catalog' };
    case 'openAgents':
      return { to: '/config/skills', label: 'Open Agents Catalog' };
    case 'openMcp':
      return { to: '/config/skills', label: 'Open MCP Catalog' };
    case 'custom':
      return action.target.startsWith('/') ? { to: action.target, label: 'Open Linked Settings' } : null;
    default:
      return null;
  }
}

const Tools: React.FC = () => {
  const navigate = useNavigate();
  const [config, setConfig] = React.useState<ApiConfigResponse | null>(null);
  const [toolDescriptors, setToolDescriptors] = React.useState<ApiToolDescriptor[]>([]);
  const [draftToolConfig, setDraftToolConfig] = React.useState<Record<string, JsonValue>>({});
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

  const loadConfig = React.useCallback(async (silent = false): Promise<void> => {
    if (silent) {
      setIsRefreshing(true);
    } else {
      setIsLoading(true);
      setError(null);
    }

    try {
      const [nextConfig, nextDescriptors] = await Promise.all([
        getConfig(),
        getToolDescriptors(),
      ]);
      const normalizedConfig = normalizeConfigResponse(nextConfig);
      const normalizedDescriptors = normalizeToolDescriptors(nextDescriptors);

      setConfig(normalizedConfig);
      setToolDescriptors(normalizedDescriptors);
      setDraftToolConfig(cloneJson(normalizedConfig.toolConfig));
      setDraftEnabledTools(new Set(normalizedConfig.enabledTools));
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

  const descriptorById = React.useMemo(
    () => Object.fromEntries(toolDescriptors.map((descriptor) => [descriptor.id, descriptor])),
    [toolDescriptors],
  );

  const toolIds = React.useMemo(() => {
    if (!config) {
      return [];
    }
    return normalizeToolIds(config, toolDescriptors);
  }, [config, toolDescriptors]);

  const tools = React.useMemo(() => {
    if (!config) {
      return [];
    }

    return toolIds.map((id) =>
      buildToolViewModel(id, draftEnabledTools, draftToolConfig, config.toolSettings, descriptorById[id] ?? null),
    );
  }, [config, toolIds, draftEnabledTools, draftToolConfig, descriptorById]);

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

      const haystack = [tool.id, tool.name, tool.description, tool.version, tool.category, ...tool.capabilities]
        .join(' ')
        .toLowerCase();

      return haystack.includes(loweredSearch);
    });
  }, [tools, filter, searchTerm]);

  const selectedTool = React.useMemo(
    () => tools.find((tool) => tool.id === selectedToolId) ?? null,
    [tools, selectedToolId],
  );

  const selectedDescriptor = React.useMemo(
    () => (selectedToolId ? descriptorById[selectedToolId] ?? null : null),
    [descriptorById, selectedToolId],
  );

  const selectedConfig = React.useMemo<JsonObject>(() => {
    if (!selectedToolId) {
      return {};
    }
    const raw = draftToolConfig[selectedToolId];
    return isJsonObject(raw) ? raw : {};
  }, [selectedToolId, draftToolConfig]);

  const selectedSettingsRaw = React.useMemo(
    () => JSON.stringify(selectedConfig, null, 2),
    [selectedConfig],
  );

  const fieldSections = React.useMemo(
    () => (selectedToolId ? buildFieldSections(selectedToolId, selectedDescriptor) : []),
    [selectedDescriptor, selectedToolId],
  );

  const hasSelectedUnsavedChanges = React.useMemo(() => {
    if (!config || !selectedToolId) {
      return false;
    }

    const savedConfig = config.toolConfig[selectedToolId] ?? {};
    const currentConfig = draftToolConfig[selectedToolId] ?? {};
    const enabledChanged =
      config.enabledTools.includes(selectedToolId) !== draftEnabledTools.has(selectedToolId);

    return !jsonEquals(savedConfig, currentConfig) || enabledChanged;
  }, [config, selectedToolId, draftEnabledTools, draftToolConfig]);

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
      ...Object.keys(config.toolConfig),
      ...Object.keys(draftToolConfig),
    ]);

    for (const toolId of allToolIds) {
      const saved = config.toolConfig[toolId] ?? {};
      const draft = draftToolConfig[toolId] ?? {};
      if (!jsonEquals(saved, draft)) {
        return true;
      }
    }

    return false;
  }, [config, draftEnabledTools, draftToolConfig]);

  const handleOpenTool = React.useCallback(
    (toolId: string) => {
      const initialConfig = isJsonObject(draftToolConfig[toolId]) ? (draftToolConfig[toolId] as JsonObject) : {};
      setSelectedToolId(toolId);
      setRawView(false);
      setRawEditorText(JSON.stringify(initialConfig, null, 2));
      setRawEditorError(null);
      setDrawerOpen(true);
    },
    [draftToolConfig],
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
    (field: ApiToolField, nextValue: JsonValue | undefined) => {
      if (!selectedToolId) {
        return;
      }

      const relativePointer = relativeToolPointer(selectedToolId, field.path);
      if (!relativePointer) {
        return;
      }

      setDraftToolConfig((previous) => {
        const toolConfig = isJsonObject(previous[selectedToolId]) ? (previous[selectedToolId] as JsonObject) : {};
        return {
          ...previous,
          [selectedToolId]: setValueAtPointer(toolConfig, relativePointer, nextValue),
        };
      });
    },
    [selectedToolId],
  );

  const handleRevertSelected = React.useCallback(() => {
    if (!config || !selectedToolId) {
      return;
    }

    setDraftToolConfig((previous) => ({
      ...previous,
      [selectedToolId]: cloneJson(config.toolConfig[selectedToolId] ?? {}),
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

    setRawEditorText(JSON.stringify(config.toolConfig[selectedToolId] ?? {}, null, 2));
    setRawEditorError(null);
  }, [config, selectedToolId]);

  const handleApplyChanges = React.useCallback(async (): Promise<void> => {
    if (!config || !selectedToolId) {
      return;
    }

    setIsSaving(true);
    setError(null);

    try {
      const updated = normalizeConfigResponse(
        await updateConfig({
          enabledTools: Array.from(draftEnabledTools).sort((a, b) => a.localeCompare(b)),
          toolConfig: cloneJson(draftToolConfig),
        }),
      );

      setConfig(updated);
      setDraftToolConfig(cloneJson(updated.toolConfig));
      setDraftEnabledTools(new Set(updated.enabledTools));
      setRawEditorError(null);
    } catch (saveError) {
      setError(formatApiError(saveError));
    } finally {
      setIsSaving(false);
    }
  }, [config, selectedToolId, draftEnabledTools, draftToolConfig]);

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
    (field: ApiToolField) => {
      if (!selectedToolId) {
        return null;
      }

      if (field.kind.type === 'action') {
        const target = actionTarget(field.kind.action);
        const helperText = field.help.trim();

        return (
          <div
            key={field.id}
            className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)]/60 p-3"
          >
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="space-y-1">
                <p className="text-sm font-semibold text-[var(--text-primary)]">{field.label}</p>
                {helperText && <p className="text-xs text-[var(--text-secondary)]">{helperText}</p>}
              </div>
              {target ? (
                <Button
                  className="border-[var(--border)] bg-[var(--bg-card)] text-xs"
                  onClick={() => navigate(target.to)}
                  type="button"
                >
                  {target.label}
                </Button>
              ) : (
                <span className="text-xs text-[var(--text-muted)]">No linked destination yet.</span>
              )}
            </div>
          </div>
        );
      }

      const relativePointer = relativeToolPointer(selectedToolId, field.path);
      const currentValue = relativePointer ? getValueAtPointer(selectedConfig, relativePointer) : undefined;
      const defaultValue = defaultValueForField(field.default);
      const helperLines = [field.help.trim(), relativePointer ? `Config path: ${relativePointer}` : null]
        .filter(Boolean)
        .join(' ');
      const defaultLabel = formatFieldDefault(field.default);

      if (field.kind.type === 'bool') {
        const selectValue =
          typeof currentValue === 'boolean'
            ? String(currentValue)
            : typeof defaultValue === 'boolean'
              ? '__default__'
              : '__unset__';

        return (
          <div key={field.id} className="space-y-1.5">
            <label className="text-xs font-semibold text-[var(--text-secondary)]">{field.label}</label>
            <select
              aria-label={field.label}
              className="h-10 w-full rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40"
              value={selectValue}
              onChange={(event) => {
                const raw = event.target.value;
                if (raw === 'true') {
                  handleFieldChange(field, true);
                } else if (raw === 'false') {
                  handleFieldChange(field, false);
                } else {
                  handleFieldChange(field, undefined);
                }
              }}
            >
              {typeof defaultValue === 'boolean' ? (
                <option value="__default__">Default ({defaultValue ? 'true' : 'false'})</option>
              ) : (
                <option value="__unset__">Unset</option>
              )}
              <option value="true">True</option>
              <option value="false">False</option>
            </select>
            {helperLines && <p className="text-xs text-[var(--text-muted)]">{helperLines}</p>}
          </div>
        );
      }

      if (field.kind.type === 'enum') {
        const options = field.kind.options;
        const selectValue =
          typeof currentValue === 'string'
            ? currentValue
            : typeof defaultValue === 'string'
              ? '__default__'
              : options[0] ?? '__unset__';

        return (
          <div key={field.id} className="space-y-1.5">
            <label className="text-xs font-semibold text-[var(--text-secondary)]">{field.label}</label>
            <select
              aria-label={field.label}
              className="h-10 w-full rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40"
              value={selectValue}
              onChange={(event) => {
                const raw = event.target.value;
                if (raw === '__default__') {
                  handleFieldChange(field, undefined);
                  return;
                }
                handleFieldChange(field, raw);
              }}
            >
              {typeof defaultValue === 'string' && (
                <option value="__default__">Default ({defaultValue})</option>
              )}
              {options.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
            {helperLines && <p className="text-xs text-[var(--text-muted)]">{helperLines}</p>}
          </div>
        );
      }

      if (field.kind.type === 'number') {
        const displayValue = typeof currentValue === 'number' ? String(currentValue) : '';
        return (
          <div key={field.id} className="space-y-1.5">
            <label className="text-xs font-semibold text-[var(--text-secondary)]">{field.label}</label>
            <input
              aria-label={field.label}
              className="h-10 w-full rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40"
              placeholder={defaultLabel ?? 'Enter a number'}
              type="number"
              value={displayValue}
              onChange={(event) => {
                const next = event.target.value.trim();
                if (next.length === 0) {
                  handleFieldChange(field, undefined);
                  return;
                }
                const parsed = Number(next);
                if (!Number.isNaN(parsed)) {
                  handleFieldChange(field, parsed);
                }
              }}
            />
            {helperLines && <p className="text-xs text-[var(--text-muted)]">{helperLines}</p>}
          </div>
        );
      }

      if (field.kind.type === 'array') {
        const displayValue = Array.isArray(currentValue)
          ? currentValue.filter((entry): entry is string => typeof entry === 'string').join(', ')
          : '';
        return (
          <div key={field.id} className="space-y-1.5">
            <label className="text-xs font-semibold text-[var(--text-secondary)]">{field.label}</label>
            <input
              aria-label={field.label}
              className="h-10 w-full rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40"
              placeholder={defaultLabel ?? 'item-a, item-b'}
              value={displayValue}
              onChange={(event) => {
                const next = event.target.value
                  .split(',')
                  .map((entry) => entry.trim())
                  .filter(Boolean);
                handleFieldChange(field, next.length > 0 ? next : undefined);
              }}
            />
            {helperLines && <p className="text-xs text-[var(--text-muted)]">{helperLines}</p>}
          </div>
        );
      }

      const stringValue = typeof currentValue === 'string' ? currentValue : '';
      return (
        <div key={field.id} className="space-y-1.5">
          <label className="text-xs font-semibold text-[var(--text-secondary)]">{field.label}</label>
          <input
            aria-label={field.label}
            className="h-10 w-full rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40"
            placeholder={defaultLabel ?? 'Enter a value'}
            value={stringValue}
            onChange={(event) => {
              const next = event.target.value;
              handleFieldChange(field, next.trim().length > 0 ? next : undefined);
            }}
          />
          {helperLines && <p className="text-xs text-[var(--text-muted)]">{helperLines}</p>}
        </div>
      );
    },
    [navigate, selectedToolId, selectedConfig, handleFieldChange],
  );

  const hasBlockingEditorErrors = rawView && rawEditorError !== null;

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
              Configure every descriptor-backed tool setting from the same schema the TUI uses, including model choices,
              runtime toggles, catalog links, and raw JSON overrides.
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
              Refresh Schema
            </Button>
          </div>
        </div>

        <div className="mt-5 flex flex-wrap items-center gap-3">
          <div className="relative min-w-[230px] flex-1">
            <SearchIcon className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--text-muted)]" />
            <input
              className="h-10 w-full rounded-lg border border-[var(--border)] bg-[var(--bg-card)] pl-10 pr-3 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40"
              placeholder="Search tools, fields, capabilities, category"
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
                <div className="space-y-1">
                  <h2 className="text-lg font-semibold text-[var(--text-primary)]">{tool.name}</h2>
                  <p className="text-xs uppercase tracking-wide text-[var(--text-muted)]">
                    {tool.id} · v{tool.version}
                  </p>
                  <p className="line-clamp-2 text-sm text-[var(--text-secondary)]">{tool.description}</p>
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
        widthClassName="w-full max-w-3xl"
      >
        {!selectedTool ? (
          <p className="text-sm text-[var(--text-secondary)]">Select a tool card to inspect and edit settings.</p>
        ) : (
          <div className="space-y-5">
            <div className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-4">
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="space-y-1">
                  <p className="text-sm font-semibold text-[var(--text-primary)]">Schema-Driven Settings</p>
                  <p className="text-xs text-[var(--text-secondary)]">
                    {selectedDescriptor?.description ?? 'Edit descriptor-backed configuration fields for this adapter.'}
                  </p>
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

              <div className="mt-4 grid gap-3 rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)]/60 p-3 sm:grid-cols-3">
                <div>
                  <p className="text-[11px] uppercase tracking-[0.2em] text-[var(--text-muted)]">Fields</p>
                  <p className="mt-1 text-lg font-semibold text-[var(--text-primary)]">
                    {selectedDescriptor?.fields.length ?? 0}
                  </p>
                </div>
                <div>
                  <p className="text-[11px] uppercase tracking-[0.2em] text-[var(--text-muted)]">Sections</p>
                  <p className="mt-1 text-lg font-semibold text-[var(--text-primary)]">{fieldSections.length}</p>
                </div>
                <div>
                  <p className="text-[11px] uppercase tracking-[0.2em] text-[var(--text-muted)]">Unsaved</p>
                  <p className="mt-1 text-lg font-semibold text-[var(--text-primary)]">
                    {hasSelectedUnsavedChanges ? 'Yes' : 'No'}
                  </p>
                </div>
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

                      setDraftToolConfig((previous) => ({
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
            ) : fieldSections.length === 0 ? (
              <section className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-5 text-sm text-[var(--text-secondary)]">
                No schema-backed fields are defined for this tool yet. You can still edit the raw JSON config above.
              </section>
            ) : (
              <div className="space-y-4">
                {fieldSections.map((section) => (
                  <section
                    key={section.id}
                    className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-4"
                  >
                    <div className="mb-3 flex items-center justify-between gap-3">
                      <h3 className="text-sm font-semibold text-[var(--text-primary)]">{section.title}</h3>
                      <span className="text-[11px] uppercase tracking-[0.18em] text-[var(--text-muted)]">
                        {section.fields.length} field{section.fields.length === 1 ? '' : 's'}
                      </span>
                    </div>
                    <div className={cn('grid gap-3', section.id === 'actions' ? 'grid-cols-1' : 'sm:grid-cols-2')}>
                      {section.fields.map((field) => renderField(field))}
                    </div>
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
