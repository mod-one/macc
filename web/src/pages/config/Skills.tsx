import React from 'react';
import { ApiClientError, getConfig, updateConfig } from '../../api/client';
import type { ApiConfigResponse, ApiConfigUpdateRequest } from '../../api/models';
import { Button, ConfirmDialog, ErrorBanner, LoadingSpinner } from '../../components';
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
      const nextConfig = await getConfig();
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
    const nextConfig = await updateConfig(request);
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

    if (!config) {
      return merged;
    }

    const addMissing = (kind: CatalogKind, id: string): void => {
      const key = itemKey(kind, id);
      if (merged.some((entry) => itemKey(entry.kind, entry.id) === key)) {
        return;
      }
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
    if (!config) {
      return new Set<string>();
    }

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
        if (tool.trim().length > 0) {
          tools.add(tool);
        }
      }
    }
    return Array.from(tools).sort((left, right) => left.localeCompare(right));
  }, [catalog]);

  const filteredItems = React.useMemo(() => {
    const search = searchTerm.trim().toLowerCase();
    return catalog.filter((item) => {
      const key = itemKey(item.kind, item.id);
      const installed = installedKeys.has(key);
      if (kindFilter !== 'all' && item.kind !== kindFilter) {
        return false;
      }
      if (toolFilter !== 'all' && !item.toolCompatibility.includes(toolFilter)) {
        return false;
      }
      if (installedFilter === 'installed' && !installed) {
        return false;
      }
      if (installedFilter === 'not-installed' && installed) {
        return false;
      }
      if (search.length === 0) {
        return true;
      }
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
      if (!config) {
        return;
      }

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

      if (draft.kind === 'skill' && !nextSkills.includes(targetId)) {
        nextSkills.push(targetId);
      }
      if (draft.kind === 'agent' && !nextAgents.includes(targetId)) {
        nextAgents.push(targetId);
      }
      if (draft.kind === 'mcp' && !nextMcp.includes(targetId)) {
        nextMcp.push(targetId);
      }

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
            // Ignore invalid custom configuration payload.
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
    if (!config || !removeCandidate) {
      return;
    }

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

  return (
    <div className="flex flex-col gap-6">
      <header className="rounded-[2rem] border border-slate-200 bg-white p-6 shadow-sm">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h1 className="text-5xl font-semibold tracking-tight text-slate-950">Skills &amp; Catalog</h1>
            <p className="mt-3 text-base text-slate-600">
              Browse skills, agents, and MCP servers. Review permissions before install.
            </p>
          </div>
          <div className="flex items-center gap-2">
            <Button className="border-slate-300 bg-slate-100 text-slate-800 hover:bg-slate-200" onClick={() => void loadConfig()}>
              Refresh
            </Button>
            <Button className="border-slate-900 bg-slate-900 text-white hover:bg-slate-700" onClick={openAddByUrl}>
              Add by URL
            </Button>
          </div>
        </div>
      </header>

      {error && <ErrorBanner message={error} title="Catalog Error" />}

      <section className="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
        <div className="grid grid-cols-1 gap-3 md:grid-cols-4">
          <label className="flex flex-col gap-1 text-xs font-medium uppercase tracking-wide text-slate-500">
            Search
            <input className="rounded-lg border border-slate-300 px-3 py-2 text-sm text-slate-900 outline-none focus:border-slate-900" onChange={(event) => setSearchTerm(event.target.value)} placeholder="Search by name, id, tool or URL" value={searchTerm} />
          </label>
          <label className="flex flex-col gap-1 text-xs font-medium uppercase tracking-wide text-slate-500">
            Kind
            <select className="rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 outline-none focus:border-slate-900" onChange={(event) => setKindFilter(event.target.value as 'all' | CatalogKind)} value={kindFilter}>
              <option value="all">All</option>
              <option value="skill">Skill</option>
              <option value="agent">Agent</option>
              <option value="mcp">MCP</option>
            </select>
          </label>
          <label className="flex flex-col gap-1 text-xs font-medium uppercase tracking-wide text-slate-500">
            Tool
            <select className="rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 outline-none focus:border-slate-900" onChange={(event) => setToolFilter(event.target.value)} value={toolFilter}>
              <option value="all">All tools</option>
              {availableTools.map((tool) => (
                <option key={tool} value={tool}>
                  {tool}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-1 text-xs font-medium uppercase tracking-wide text-slate-500">
            Installed
            <select className="rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 outline-none focus:border-slate-900" onChange={(event) => setInstalledFilter(event.target.value as InstalledFilter)} value={installedFilter}>
              <option value="all">All</option>
              <option value="installed">Installed</option>
              <option value="not-installed">Not installed</option>
            </select>
          </label>
        </div>
      </section>

      <section className="grid grid-cols-1 gap-4 xl:grid-cols-2">
        {filteredItems.length === 0 && <div className="rounded-2xl border border-slate-200 bg-white p-6 text-sm text-slate-600">No catalog items match the current filters.</div>}

        {filteredItems.map((item) => {
          const key = itemKey(item.kind, item.id);
          const installed = installedKeys.has(key);
          const cached = cachedItemKeys.has(key);

          return (
            <article key={key} className="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm">
              <div className="flex items-start justify-between gap-3">
                <div>
                  <h2 className="text-lg font-semibold text-slate-950">{item.name}</h2>
                  <p className="text-xs uppercase tracking-wide text-slate-500">{item.id}</p>
                </div>
                <div className="flex flex-wrap items-center gap-2 text-xs">
                  <span className="rounded-full border border-slate-300 px-2 py-0.5 text-slate-600">{kindLabel(item.kind)}</span>
                  <span className={`rounded-full px-2 py-0.5 ${item.verified ? 'bg-emerald-100 text-emerald-700' : 'bg-amber-100 text-amber-700'}`}>{item.verified ? 'Verified' : 'Unverified'}</span>
                  <span className="rounded-full bg-slate-100 px-2 py-0.5 text-slate-700">{item.sourceKind}</span>
                  <span className={`rounded-full px-2 py-0.5 ${cached ? 'bg-blue-100 text-blue-700' : 'bg-slate-100 text-slate-500'}`}>Cache: {cached ? 'cached' : 'not cached'}</span>
                </div>
              </div>
              <p className="mt-3 text-sm text-slate-600">{item.description}</p>
              <div className="mt-3 flex flex-wrap gap-2">
                {item.toolCompatibility.length > 0 ? item.toolCompatibility.map((tool) => <span key={tool} className="rounded-full border border-slate-300 bg-slate-50 px-2 py-0.5 text-xs text-slate-700">{tool}</span>) : <span className="rounded-full border border-slate-300 bg-slate-50 px-2 py-0.5 text-xs text-slate-500">No tool compatibility metadata</span>}
              </div>
              <div className="mt-4 flex items-center justify-between gap-2">
                <span className={`rounded-full px-2 py-0.5 text-xs ${installed ? 'bg-emerald-100 text-emerald-700' : 'bg-slate-100 text-slate-600'}`}>{installed ? 'Installed' : 'Not installed'}</span>
                <div className="flex items-center gap-2">
                  <Button className="border-slate-300 bg-slate-100 text-slate-800 hover:bg-slate-200" onClick={() => setInstallDraft(toInstallDraft(item))}>Review</Button>
                  {installed ? (
                    <Button className="border-rose-300 bg-rose-100 text-rose-700 hover:bg-rose-200" onClick={() => setRemoveCandidate({ kind: item.kind, id: item.id })}>Remove</Button>
                  ) : (
                    <Button className="border-slate-900 bg-slate-900 text-white hover:bg-slate-700" onClick={() => setInstallDraft(toInstallDraft(item))}>Install</Button>
                  )}
                </div>
              </div>
            </article>
          );
        })}
      </section>

      <SkillsInstallModal draft={installDraft} isSaving={isSaving} onClose={() => setInstallDraft(null)} onInstall={(manifestText, draft) => void onInstall(manifestText, draft)} />

      <ConfirmDialog
        confirmationPhrase={removeCandidate?.id ?? 'CONFIRM'}
        description={removeCandidate ? `Remove ${removeCandidate.id} from selected ${kindLabel(removeCandidate.kind).toLowerCase()} entries?` : 'Remove selected package?'}
        intent="danger"
        onConfirm={() => void removeInstalledItem()}
        onOpenChange={(open) => {
          if (!open) {
            setRemoveCandidate(null);
          }
        }}
        open={Boolean(removeCandidate)}
        title="Remove installed item"
      />
    </div>
  );
};

export default Skills;
