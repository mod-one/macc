import React from 'react';
import { ApiClientError, getConfig, updateConfig } from '../../api/client';
import type { ApiConfigResponse, ApiConfigUpdateRequest } from '../../api/models';
import { Button, ConfirmDialog, ErrorBanner, LoadingSpinner } from '../../components';
import { SearchIcon, PlusIcon, RefreshIcon, CheckIcon, XIcon } from '../../components/icons';
import { cn } from '../../components/styles';
import SkillsInstallModal from './SkillsInstallModal';
import {
  CACHED_ITEMS_STORAGE_KEY,
  SEED_CATALOG,
  itemKey,
  kindLabel,
  normalizeToolList,
  readStoredCatalog,
  readStoredStringArray,
  toInstallDraft,
  upsertCatalogItem,
  writeStoredCatalog,
  writeStoredStringArray,
  hasPostInstallScripts,
  type CatalogItem,
  type CatalogKind,
  type InstallDraft,
  type InstalledFilter,
} from './skillsCatalog';

function formatError(error: unknown): string {
  if (error instanceof ApiClientError) {
    return `${error.envelope.error.code}: ${error.envelope.error.message}`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return 'Unexpected catalog error.';
}

function ensureStringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === 'string') : [];
}

function normalizeConfigResponse(config: ApiConfigResponse): ApiConfigResponse {
  return {
    ...config,
    enabledTools: ensureStringArray(config.enabledTools),
    selectedSkills: ensureStringArray(config.selectedSkills),
    selectedAgents: ensureStringArray(config.selectedAgents),
    selectedMcp: ensureStringArray(config.selectedMcp),
    toolPriority: ensureStringArray(config.toolPriority),
    managedEnvironmentWarnings: ensureStringArray(config.managedEnvironmentWarnings),
  };
}

function kindChipStyle(kind: CatalogKind): React.CSSProperties {
  switch (kind) {
    case 'skill':
      return {
        backgroundColor: 'color-mix(in oklch, var(--accent) 16%, transparent)',
        color: 'var(--accent)',
      };
    case 'agent':
      return {
        backgroundColor: 'oklch(0.65 0.15 300 / 0.16)',
        color: 'oklch(0.72 0.14 300)',
      };
    case 'mcp':
      return {
        backgroundColor: 'oklch(0.75 0.17 80 / 0.16)',
        color: 'var(--warning)',
      };
  }
}

const Skills: React.FC = () => {
  const [config, setConfig] = React.useState<ApiConfigResponse | null>(null);
  const [customCatalog, setCustomCatalog] = React.useState<CatalogItem[]>([]);
  const [cachedItemKeys, setCachedItemKeys] = React.useState<Set<string>>(new Set());
  const [isLoading, setIsLoading] = React.useState(true);
  const [isSaving, setIsSaving] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const [searchTerm, setSearchTerm] = React.useState('');
  const [kindFilter, setKindFilter] = React.useState<'all' | CatalogKind>('all');
  const [toolFilter, setToolFilter] = React.useState<'all' | string>('all');
  const [installedFilter, setInstalledFilter] = React.useState<InstalledFilter>('all');

  const [installDraft, setInstallDraft] = React.useState<InstallDraft | null>(null);
  const [removeCandidate, setRemoveCandidate] = React.useState<{ kind: CatalogKind; id: string } | null>(null);

  const loadConfig = React.useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const nextConfig = normalizeConfigResponse(await getConfig());
      setConfig(nextConfig);
      setCustomCatalog(readStoredCatalog());
      setCachedItemKeys(new Set(readStoredStringArray(CACHED_ITEMS_STORAGE_KEY)));
    } catch (loadError) {
      setError(formatError(loadError));
    } finally {
      setIsLoading(false);
    }
  }, []);

  React.useEffect(() => {
    void loadConfig();
  }, [loadConfig]);

  const saveConfigSelection = React.useCallback(async (request: ApiConfigUpdateRequest) => {
    const nextConfig = normalizeConfigResponse(await updateConfig(request));
    setConfig(nextConfig);
    return nextConfig;
  }, []);

  const catalog = React.useMemo(() => {
    const merged = [...SEED_CATALOG];
    for (const item of customCatalog) {
      const key = itemKey(item.kind, item.id);
      if (!merged.some((entry) => itemKey(entry.kind, entry.id) === key)) {
        merged.push(item);
      }
    }

    if (!config) return merged;

    const addMissing = (kind: CatalogKind, id: string): void => {
      const key = itemKey(kind, id);
      if (merged.some((entry) => itemKey(entry.kind, entry.id) === key)) return;
      merged.push({
        id,
        name: id,
        description: `Installed ${kindLabel(kind).toLowerCase()} from project configuration.`,
        kind,
        toolCompatibility: [],
        verified: false,
        sourceKind: 'remote',
        security: { env: kind === 'agent', network: false, fs: true },
        configuration: {},
        manifest:
          kind === 'mcp' ? { type: kind, id, merge_target: `mcpServers.${id}` } : { type: kind, id },
      });
    };

    config.selectedSkills.forEach((id) => addMissing('skill', id));
    config.selectedAgents.forEach((id) => addMissing('agent', id));
    config.selectedMcp.forEach((id) => addMissing('mcp', id));

    return merged;
  }, [config, customCatalog]);

  const installedKeys = React.useMemo(() => {
    if (!config) return new Set<string>();
    return new Set<string>([
      ...config.selectedSkills.map((id) => itemKey('skill', id)),
      ...config.selectedAgents.map((id) => itemKey('agent', id)),
      ...config.selectedMcp.map((id) => itemKey('mcp', id)),
    ]);
  }, [config]);

  const availableTools = React.useMemo(() => {
    const tools = new Set<string>();
    for (const item of catalog) {
      for (const tool of item.toolCompatibility) {
        if (tool.trim().length > 0) tools.add(tool);
      }
    }
    return Array.from(tools).sort((a, b) => a.localeCompare(b));
  }, [catalog]);

  const filteredItems = React.useMemo(() => {
    const search = searchTerm.trim().toLowerCase();
    return catalog.filter((item) => {
      const key = itemKey(item.kind, item.id);
      const installed = installedKeys.has(key);
      if (kindFilter !== 'all' && item.kind !== kindFilter) return false;
      if (toolFilter !== 'all' && !item.toolCompatibility.includes(toolFilter)) return false;
      if (installedFilter === 'installed' && !installed) return false;
      if (installedFilter === 'not-installed' && installed) return false;
      if (search.length === 0) return true;
      const haystack = [item.id, item.name, item.description, item.sourceUrl ?? '', ...item.toolCompatibility]
        .join(' ')
        .toLowerCase();
      return haystack.includes(search);
    });
  }, [catalog, installedFilter, installedKeys, kindFilter, searchTerm, toolFilter]);

  const openAddByUrl = React.useCallback(() => {
    setInstallDraft({
      id: 'custom-package',
      name: 'Custom Package',
      description: 'Remote package from URL import.',
      kind: 'skill',
      toolCompatibilityText: 'codex',
      verified: false,
      sourceKind: 'remote',
      sourceUrl: '',
      security: { env: false, network: false, fs: true },
      configurationText: '{}',
    });
  }, []);

  const onInstall = React.useCallback(
    async (manifestText: string, draft: InstallDraft) => {
      if (!config) return;

      let manifest: Record<string, unknown>;
      try {
        const parsed = JSON.parse(manifestText) as unknown;
        if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
          setError('Manifest must be a JSON object.');
          return;
        }
        manifest = parsed as Record<string, unknown>;
      } catch {
        setError('Manifest contains invalid JSON.');
        return;
      }

      if (hasPostInstallScripts(manifest)) {
        setError('Post-install scripts are not allowed. Remote packages must remain data-only.');
        return;
      }

      setIsSaving(true);
      setError(null);

      const targetId = draft.id.trim();
      const nextSkills = [...config.selectedSkills];
      const nextAgents = [...config.selectedAgents];
      const nextMcp = [...config.selectedMcp];

      if (draft.kind === 'skill' && !nextSkills.includes(targetId)) nextSkills.push(targetId);
      if (draft.kind === 'agent' && !nextAgents.includes(targetId)) nextAgents.push(targetId);
      if (draft.kind === 'mcp' && !nextMcp.includes(targetId)) nextMcp.push(targetId);

      const normalizedItem: CatalogItem = {
        id: targetId,
        name: draft.name.trim() || targetId,
        description: draft.description,
        kind: draft.kind,
        toolCompatibility: normalizeToolList(draft.toolCompatibilityText),
        verified: draft.verified,
        sourceKind: draft.sourceKind,
        sourceUrl: draft.sourceUrl.trim() || undefined,
        security: { ...draft.security },
        configuration: (() => {
          try {
            const parsed = JSON.parse(draft.configurationText) as unknown;
            if (typeof parsed === 'object' && parsed !== null && !Array.isArray(parsed)) {
              return parsed as Record<string, unknown>;
            }
          } catch {
            // ignore
          }
          return {};
        })(),
        manifest,
      };

      try {
        await saveConfigSelection({ selectedSkills: nextSkills, selectedAgents: nextAgents, selectedMcp: nextMcp });
        const nextCatalog = upsertCatalogItem(customCatalog, normalizedItem);
        setCustomCatalog(nextCatalog);
        writeStoredCatalog(nextCatalog);
        const nextCached = new Set(cachedItemKeys);
        nextCached.add(itemKey(normalizedItem.kind, normalizedItem.id));
        setCachedItemKeys(nextCached);
        writeStoredStringArray(CACHED_ITEMS_STORAGE_KEY, Array.from(nextCached));
        setInstallDraft(null);
      } catch (saveError) {
        setError(formatError(saveError));
      } finally {
        setIsSaving(false);
      }
    },
    [cachedItemKeys, config, customCatalog, saveConfigSelection],
  );

  const removeInstalledItem = React.useCallback(async () => {
    if (!config || !removeCandidate) return;
    setIsSaving(true);
    setError(null);
    const nextSkills = config.selectedSkills.filter(
      (id) => !(removeCandidate.kind === 'skill' && id === removeCandidate.id),
    );
    const nextAgents = config.selectedAgents.filter(
      (id) => !(removeCandidate.kind === 'agent' && id === removeCandidate.id),
    );
    const nextMcp = config.selectedMcp.filter(
      (id) => !(removeCandidate.kind === 'mcp' && id === removeCandidate.id),
    );
    try {
      await saveConfigSelection({ selectedSkills: nextSkills, selectedAgents: nextAgents, selectedMcp: nextMcp });
      setRemoveCandidate(null);
    } catch (saveError) {
      setError(formatError(saveError));
    } finally {
      setIsSaving(false);
    }
  }, [config, removeCandidate, saveConfigSelection]);

  if (isLoading) {
    return <LoadingSpinner label="Loading catalog..." />;
  }

  const installedCount = filteredItems.filter((item) => installedKeys.has(itemKey(item.kind, item.id))).length;
  const activeFilters = searchTerm || kindFilter !== 'all' || toolFilter !== 'all' || installedFilter !== 'all';

  const clearFilters = () => {
    setSearchTerm('');
    setKindFilter('all');
    setToolFilter('all');
    setInstalledFilter('all');
  };

  return (
    <div className="flex flex-col gap-5">
      {/* Page header */}
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold tracking-tight text-[var(--text-primary)]">
            Skills &amp; Catalog
          </h1>
          <p className="mt-0.5 text-sm text-[var(--text-secondary)]">
            Browse and install skills, agents, and MCP servers. Review permissions before installing.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            className="h-8 border-[var(--border)] bg-[var(--bg-card)] px-3 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
            onClick={() => void loadConfig()}
            type="button"
          >
            <RefreshIcon className="mr-1.5 h-3.5 w-3.5" />
            Refresh
          </Button>
          <Button
            className="h-8 px-3 text-xs"
            onClick={openAddByUrl}
            style={{
              borderColor: 'color-mix(in oklch, var(--accent) 40%, transparent)',
              backgroundColor: 'var(--accent-bg)',
              color: 'var(--accent)',
            }}
            type="button"
          >
            <PlusIcon className="mr-1.5 h-3.5 w-3.5" />
            Add by URL
          </Button>
        </div>
      </div>

      {error && <ErrorBanner message={error} title="Catalog error" />}

      {/* Filter bar */}
      <div className="flex flex-wrap items-center gap-2">
        <div className="relative min-w-[180px] flex-1">
          <SearchIcon className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--text-muted)]" />
          <input
            className="h-8 w-full rounded-md border border-[var(--border)] bg-[var(--bg-card)] pl-8 pr-3 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50"
            onChange={(event) => setSearchTerm(event.target.value)}
            placeholder="Search name, id, tool"
            value={searchTerm}
          />
        </div>

        <select
          aria-label="Filter by kind"
          className="h-8 rounded-md border border-[var(--border)] bg-[var(--bg-card)] px-2 pr-6 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50"
          onChange={(event) => setKindFilter(event.target.value as 'all' | CatalogKind)}
          value={kindFilter}
        >
          <option value="all">All types</option>
          <option value="skill">Skills</option>
          <option value="agent">Agents</option>
          <option value="mcp">MCP</option>
        </select>

        {availableTools.length > 0 && (
          <select
            aria-label="Filter by tool"
            className="h-8 rounded-md border border-[var(--border)] bg-[var(--bg-card)] px-2 pr-6 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-1 focus:ring-[var(--accent)]/50"
            onChange={(event) => setToolFilter(event.target.value)}
            value={toolFilter}
          >
            <option value="all">All tools</option>
            {availableTools.map((tool) => (
              <option key={tool} value={tool}>
                {tool}
              </option>
            ))}
          </select>
        )}

        <div className="inline-flex rounded-md border border-[var(--border)] bg-[var(--bg-card)] p-0.5" role="group">
          {(['all', 'installed', 'not-installed'] as InstalledFilter[]).map((value) => {
            const label = value === 'all' ? 'All' : value === 'installed' ? 'Installed' : 'Available';
            return (
              <button
                key={value}
                className={cn(
                  'rounded px-2.5 py-1 text-xs font-medium transition-colors',
                  installedFilter === value
                    ? 'bg-[var(--accent)] text-white'
                    : 'text-[var(--text-secondary)] hover:bg-white/8 hover:text-[var(--text-primary)]',
                )}
                onClick={() => setInstalledFilter(value)}
                type="button"
              >
                {label}
              </button>
            );
          })}
        </div>
      </div>

      {/* Results line */}
      <div className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
        <span>
          {filteredItems.length} {filteredItems.length === 1 ? 'item' : 'items'}
        </span>
        {installedCount > 0 && (
          <>
            <span aria-hidden>·</span>
            <span style={{ color: 'var(--success)' }}>{installedCount} installed</span>
          </>
        )}
        {activeFilters && (
          <>
            <span aria-hidden>·</span>
            <button
              className="hover:underline"
              onClick={clearFilters}
              style={{ color: 'var(--accent)' }}
              type="button"
            >
              Clear filters
            </button>
          </>
        )}
      </div>

      {/* Catalog list */}
      {filteredItems.length === 0 ? (
        <div
          className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] px-5 py-12 text-center"
          style={{ boxShadow: 'var(--shadow-soft)' }}
        >
          <p className="text-sm text-[var(--text-secondary)]">No items match the current filters.</p>
          {activeFilters && (
            <button
              className="mt-2 text-xs hover:underline"
              onClick={clearFilters}
              style={{ color: 'var(--accent)' }}
              type="button"
            >
              Clear filters
            </button>
          )}
        </div>
      ) : (
        <ul
          className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)]"
          style={{ boxShadow: 'var(--shadow-soft)' }}
        >
          {filteredItems.map((item, index) => {
            const key = itemKey(item.kind, item.id);
            const installed = installedKeys.has(key);

            return (
              <li
                key={key}
                className={cn(
                  'flex flex-wrap items-start gap-3 px-4 py-3 transition-colors sm:flex-nowrap',
                  index > 0 && 'border-t border-[var(--border-subtle)]',
                  installed
                    ? 'bg-[var(--accent-bg)] hover:bg-[var(--accent-bg-hover)]'
                    : 'bg-[var(--bg-card)] hover:bg-[var(--bg-elevated)]',
                )}
              >
                {/* Kind badge */}
                <span
                  className="mt-0.5 shrink-0 rounded px-2 py-0.5 text-[10px] font-semibold"
                  style={kindChipStyle(item.kind)}
                >
                  {kindLabel(item.kind)}
                </span>

                {/* Main content */}
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-baseline gap-x-2">
                    <span className="text-sm font-semibold text-[var(--text-primary)]">{item.name}</span>
                    <span
                      className="font-mono text-[11px] text-[var(--text-muted)]"
                      title={item.id}
                    >
                      {item.id}
                    </span>
                  </div>
                  <p className="mt-0.5 line-clamp-1 text-sm text-[var(--text-secondary)]">
                    {item.description}
                  </p>
                  {item.toolCompatibility.length > 0 && (
                    <div className="mt-1.5 flex flex-wrap gap-1">
                      {item.toolCompatibility.map((tool) => (
                        <span
                          key={tool}
                          className="rounded border border-[var(--border)] bg-[var(--bg-secondary)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--text-muted)]"
                        >
                          {tool}
                        </span>
                      ))}
                    </div>
                  )}
                </div>

                {/* Status + actions */}
                <div className="flex shrink-0 flex-wrap items-center gap-2 sm:ml-auto">
                  {item.verified && (
                    <span
                      className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-medium"
                      style={{
                        backgroundColor: 'oklch(0.65 0.18 145 / 0.14)',
                        color: 'var(--success)',
                      }}
                    >
                      <CheckIcon className="h-2.5 w-2.5" />
                      Verified
                    </span>
                  )}
                  {installed && (
                    <span
                      className="rounded px-1.5 py-0.5 text-[10px] font-medium"
                      style={{
                        backgroundColor: 'color-mix(in oklch, var(--accent) 14%, transparent)',
                        color: 'var(--accent)',
                      }}
                    >
                      Installed
                    </span>
                  )}

                  <Button
                    className="h-7 border-[var(--border)] bg-[var(--bg-secondary)] px-2.5 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
                    onClick={() => setInstallDraft(toInstallDraft(item))}
                    type="button"
                  >
                    Review
                  </Button>

                  {installed ? (
                    <Button
                      className="h-7 px-2.5 text-xs"
                      onClick={() => setRemoveCandidate({ kind: item.kind, id: item.id })}
                      style={{
                        borderColor: 'oklch(0.62 0.22 25 / 0.35)',
                        backgroundColor: 'oklch(0.62 0.22 25 / 0.1)',
                        color: 'var(--error)',
                      }}
                      type="button"
                    >
                      <XIcon className="mr-1 h-3 w-3" />
                      Remove
                    </Button>
                  ) : (
                    <Button
                      className="h-7 px-2.5 text-xs"
                      onClick={() => setInstallDraft(toInstallDraft(item))}
                      style={{
                        borderColor: 'color-mix(in oklch, var(--accent) 40%, transparent)',
                        backgroundColor: 'var(--accent-bg)',
                        color: 'var(--accent)',
                      }}
                      type="button"
                    >
                      Install
                    </Button>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}

      <SkillsInstallModal
        draft={installDraft}
        isSaving={isSaving}
        onClose={() => setInstallDraft(null)}
        onInstall={(manifestText, draft) => void onInstall(manifestText, draft)}
      />

      <ConfirmDialog
        confirmationPhrase={removeCandidate?.id ?? 'CONFIRM'}
        description={
          removeCandidate
            ? `Remove ${removeCandidate.id} from selected ${kindLabel(removeCandidate.kind).toLowerCase()} entries?`
            : 'Remove selected package?'
        }
        intent="danger"
        onConfirm={() => void removeInstalledItem()}
        onOpenChange={(open) => {
          if (!open) setRemoveCandidate(null);
        }}
        open={Boolean(removeCandidate)}
        title="Remove installed item"
      />
    </div>
  );
};

export default Skills;
