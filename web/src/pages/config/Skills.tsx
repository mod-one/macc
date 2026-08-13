import React from 'react';
import {
  ApiClientError,
  getCatalogMcpAvailable,
  getCatalogSkillsAvailable,
  getConfig,
  updateConfig,
} from '../../api/client';
import type { ApiCatalogMcpEntry, ApiCatalogSkillEntry, ApiConfigResponse, ApiConfigUpdateRequest } from '../../api/models';
import { Button, ConfirmDialog, ErrorBanner, LoadingSpinner } from '../../components';
import { CheckIcon, RefreshIcon, SearchIcon, XIcon } from '../../components/icons';
import { cn } from '../../components/styles';
import {
  itemKey,
  kindLabel,
  mcpToViewItem,
  skillToViewItem,
  type CatalogKind,
  type CatalogViewItem,
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
    mandatorySkills: ensureStringArray(config.mandatorySkills),
    selectedAgents: ensureStringArray(config.selectedAgents),
    selectedMcp: ensureStringArray(config.selectedMcp),
    toolPriority: ensureStringArray(config.toolPriority),
    managedEnvironmentWarnings: ensureStringArray(config.managedEnvironmentWarnings),
  };
}

function kindChipStyle(kind: CatalogKind): React.CSSProperties {
  return kind === 'skill'
    ? {
        backgroundColor: 'color-mix(in oklch, var(--accent) 16%, transparent)',
        color: 'var(--accent)',
      }
    : {
        backgroundColor: 'oklch(0.75 0.17 80 / 0.16)',
        color: 'var(--warning)',
      };
}

function buildCatalogView(
  skills: ApiCatalogSkillEntry[],
  mcpEntries: ApiCatalogMcpEntry[],
  config: ApiConfigResponse | null,
): CatalogViewItem[] {
  const selectedSkills = new Set(config?.selectedSkills ?? []);
  const mandatorySkills = new Set(config?.mandatorySkills ?? []);
  const selectedMcp = new Set(config?.selectedMcp ?? []);

  return [
    ...skills.map((entry) => skillToViewItem(entry, selectedSkills, mandatorySkills)),
    ...mcpEntries.map((entry) => mcpToViewItem(entry, selectedMcp)),
  ].sort((a, b) => {
    if (a.kind !== b.kind) return a.kind === 'skill' ? -1 : 1;
    return a.name.localeCompare(b.name);
  });
}

const Skills: React.FC = () => {
  const [config, setConfig] = React.useState<ApiConfigResponse | null>(null);
  const [skillsCatalog, setSkillsCatalog] = React.useState<ApiCatalogSkillEntry[]>([]);
  const [mcpCatalog, setMcpCatalog] = React.useState<ApiCatalogMcpEntry[]>([]);
  const [isLoading, setIsLoading] = React.useState(true);
  const [isSaving, setIsSaving] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const [searchTerm, setSearchTerm] = React.useState('');
  const [kindFilter, setKindFilter] = React.useState<'all' | CatalogKind>('all');
  const [toolFilter, setToolFilter] = React.useState<'all' | string>('all');
  const [installedFilter, setInstalledFilter] = React.useState<InstalledFilter>('all');
  const [removeCandidate, setRemoveCandidate] = React.useState<CatalogViewItem | null>(null);

  const loadCatalog = React.useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const [nextConfig, nextSkills, nextMcp] = await Promise.all([
        getConfig(),
        getCatalogSkillsAvailable(),
        getCatalogMcpAvailable(),
      ]);
      setConfig(normalizeConfigResponse(nextConfig));
      setSkillsCatalog(Array.isArray(nextSkills.skills) ? nextSkills.skills : []);
      setMcpCatalog(Array.isArray(nextMcp.mcp) ? nextMcp.mcp : []);
    } catch (loadError) {
      setError(formatError(loadError));
    } finally {
      setIsLoading(false);
    }
  }, []);

  React.useEffect(() => {
    void loadCatalog();
  }, [loadCatalog]);

  const saveConfigSelection = React.useCallback(async (request: ApiConfigUpdateRequest) => {
    const nextConfig = normalizeConfigResponse(await updateConfig(request));
    setConfig(nextConfig);
    return nextConfig;
  }, []);

  const catalog = React.useMemo(
    () => buildCatalogView(skillsCatalog, mcpCatalog, config),
    [config, mcpCatalog, skillsCatalog],
  );

  const availableTools = React.useMemo(() => {
    const tools = new Set<string>();
    for (const item of catalog) {
      for (const tool of item.tools) {
        if (tool.trim().length > 0) tools.add(tool);
      }
    }
    return Array.from(tools).sort((a, b) => a.localeCompare(b));
  }, [catalog]);

  const filteredItems = React.useMemo(() => {
    const search = searchTerm.trim().toLowerCase();
    return catalog.filter((item) => {
      if (kindFilter !== 'all' && item.kind !== kindFilter) return false;
      if (toolFilter !== 'all' && !item.tools.includes(toolFilter)) return false;
      if (installedFilter === 'installed' && !item.installed) return false;
      if (installedFilter === 'not-installed' && item.installed) return false;
      if (search.length === 0) return true;
      const haystack = [item.id, item.name, item.description, item.sourceUrl, ...item.tags, ...item.tools]
        .join(' ')
        .toLowerCase();
      return haystack.includes(search);
    });
  }, [catalog, installedFilter, kindFilter, searchTerm, toolFilter]);

  const installItem = React.useCallback(
    async (item: CatalogViewItem) => {
      if (!config) return;
      setIsSaving(true);
      setError(null);

      const nextSkills =
        item.kind === 'skill' && !config.selectedSkills.includes(item.id)
          ? [...config.selectedSkills, item.id]
          : config.selectedSkills;
      const nextMcp =
        item.kind === 'mcp' && !config.selectedMcp.includes(item.id)
          ? [...config.selectedMcp, item.id]
          : config.selectedMcp;

      try {
        await saveConfigSelection({ selectedSkills: nextSkills, selectedMcp: nextMcp });
      } catch (saveError) {
        setError(formatError(saveError));
      } finally {
        setIsSaving(false);
      }
    },
    [config, saveConfigSelection],
  );

  const removeInstalledItem = React.useCallback(async () => {
    if (!config || !removeCandidate) return;
    if (removeCandidate.kind === 'skill' && removeCandidate.mandatory) {
      setError(`Skill '${removeCandidate.id}' is mandatory and cannot be removed.`);
      setRemoveCandidate(null);
      return;
    }

    setIsSaving(true);
    setError(null);
    const nextSkills = config.selectedSkills.filter(
      (id) => !(removeCandidate.kind === 'skill' && id === removeCandidate.id),
    );
    const nextMcp = config.selectedMcp.filter(
      (id) => !(removeCandidate.kind === 'mcp' && id === removeCandidate.id),
    );

    try {
      await saveConfigSelection({ selectedSkills: nextSkills, selectedMcp: nextMcp });
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

  const installedCount = filteredItems.filter((item) => item.installed).length;
  const activeFilters = searchTerm || kindFilter !== 'all' || toolFilter !== 'all' || installedFilter !== 'all';

  const clearFilters = () => {
    setSearchTerm('');
    setKindFilter('all');
    setToolFilter('all');
    setInstalledFilter('all');
  };

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold tracking-tight text-[var(--text-primary)]">
            Skills &amp; Catalog
          </h1>
          <p className="mt-0.5 text-sm text-[var(--text-secondary)]">
            Browse catalog-backed skills and MCP servers. Selection updates project configuration.
          </p>
        </div>
        <Button
          className="h-8 border-[var(--border)] bg-[var(--bg-card)] px-3 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
          onClick={() => void loadCatalog()}
          type="button"
        >
          <RefreshIcon className="mr-1.5 h-3.5 w-3.5" />
          Refresh
        </Button>
      </div>

      {error && <ErrorBanner message={error} title="Catalog error" />}

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
            const mandatory = item.kind === 'skill' && item.mandatory;

            return (
              <li
                key={itemKey(item.kind, item.id)}
                className={cn(
                  'flex flex-wrap items-start gap-3 px-4 py-3 transition-colors sm:flex-nowrap',
                  index > 0 && 'border-t border-[var(--border-subtle)]',
                  item.installed
                    ? 'bg-[var(--accent-bg)] hover:bg-[var(--accent-bg-hover)]'
                    : 'bg-[var(--bg-card)] hover:bg-[var(--bg-elevated)]',
                )}
              >
                <span
                  className="mt-0.5 shrink-0 rounded px-2 py-0.5 text-[10px] font-semibold"
                  style={kindChipStyle(item.kind)}
                >
                  {kindLabel(item.kind)}
                </span>

                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-baseline gap-x-2">
                    <span className="text-sm font-semibold text-[var(--text-primary)]">{item.name}</span>
                    <span className="font-mono text-[11px] text-[var(--text-muted)]" title={item.id}>
                      {item.id}
                    </span>
                  </div>
                  <p className="mt-0.5 line-clamp-1 text-sm text-[var(--text-secondary)]">
                    {item.description}
                  </p>
                  <div className="mt-1.5 flex flex-wrap gap-1">
                    {item.tools.map((tool) => (
                      <span
                        key={tool}
                        className="rounded border border-[var(--border)] bg-[var(--bg-secondary)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--text-muted)]"
                      >
                        {tool}
                      </span>
                    ))}
                    <span className="rounded border border-[var(--border)] bg-[var(--bg-secondary)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--text-muted)]">
                      {item.sourceKind}
                    </span>
                  </div>
                </div>

                <div className="flex shrink-0 flex-wrap items-center gap-2 sm:ml-auto">
                  {item.installed && (
                    <span
                      className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-medium"
                      style={{
                        backgroundColor: 'color-mix(in oklch, var(--accent) 14%, transparent)',
                        color: 'var(--accent)',
                      }}
                    >
                      <CheckIcon className="h-2.5 w-2.5" />
                      Installed
                    </span>
                  )}
                  {mandatory && (
                    <span
                      className="rounded px-1.5 py-0.5 text-[10px] font-medium"
                      style={{
                        backgroundColor: 'oklch(0.75 0.17 80 / 0.14)',
                        color: 'var(--warning)',
                      }}
                    >
                      Mandatory
                    </span>
                  )}

                  {item.installed ? (
                    <Button
                      className="h-7 px-2.5 text-xs"
                      disabled={mandatory || isSaving}
                      onClick={() => {
                        if (!mandatory) setRemoveCandidate(item);
                      }}
                      style={{
                        borderColor: 'oklch(0.62 0.22 25 / 0.35)',
                        backgroundColor: mandatory ? 'var(--bg-secondary)' : 'oklch(0.62 0.22 25 / 0.1)',
                        color: mandatory ? 'var(--text-muted)' : 'var(--error)',
                      }}
                      type="button"
                    >
                      <XIcon className="mr-1 h-3 w-3" />
                      Remove
                    </Button>
                  ) : (
                    <Button
                      className="h-7 px-2.5 text-xs"
                      disabled={isSaving}
                      onClick={() => void installItem(item)}
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
