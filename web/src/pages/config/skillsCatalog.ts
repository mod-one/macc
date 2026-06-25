export type CatalogKind = 'skill' | 'agent' | 'mcp';
export type SourceKind = 'builtin' | 'registry' | 'remote';
export type InstalledFilter = 'all' | 'installed' | 'not-installed';

export interface CatalogSecurity {
  env: boolean;
  network: boolean;
  fs: boolean;
}

export interface CatalogItem {
  id: string;
  name: string;
  description: string;
  kind: CatalogKind;
  toolCompatibility: string[];
  verified: boolean;
  mandatory?: boolean;
  sourceKind: SourceKind;
  sourceUrl?: string;
  security: CatalogSecurity;
  configuration: Record<string, unknown>;
  manifest: Record<string, unknown>;
}

export interface InstallDraft {
  id: string;
  name: string;
  description: string;
  kind: CatalogKind;
  toolCompatibilityText: string;
  verified: boolean;
  sourceKind: SourceKind;
  sourceUrl: string;
  security: CatalogSecurity;
  configurationText: string;
}

export const CUSTOM_CATALOG_STORAGE_KEY = 'macc.web.skills.customCatalog.v1';
export const CACHED_ITEMS_STORAGE_KEY = 'macc.web.skills.cachedItems.v1';

export const SEED_CATALOG: CatalogItem[] = [
  {
    id: 'macc-performer',
    name: 'MACC Performer',
    description: 'Task-scoped implementation performer for MACC worktrees.',
    kind: 'skill',
    toolCompatibility: ['codex', 'claude'],
    verified: true,
    mandatory: true,
    sourceKind: 'builtin',
    security: { env: false, network: false, fs: true },
    configuration: { mode: 'worktree' },
    manifest: { type: 'skill', id: 'macc-performer' },
  },
  {
    id: 'macc-reviewer',
    name: 'MACC Reviewer',
    description: 'Structured review skill focused on regressions and risk.',
    kind: 'skill',
    toolCompatibility: ['codex', 'gemini'],
    verified: true,
    mandatory: true,
    sourceKind: 'builtin',
    security: { env: false, network: false, fs: true },
    configuration: { mode: 'review' },
    manifest: { type: 'skill', id: 'macc-reviewer' },
  },
  {
    id: 'planner-agent',
    name: 'Planner Agent',
    description: 'Specialized planning agent for PRD and execution sequencing.',
    kind: 'agent',
    toolCompatibility: ['codex'],
    verified: true,
    sourceKind: 'registry',
    security: { env: true, network: false, fs: true },
    configuration: { temperature: 0.2 },
    manifest: { type: 'agent', id: 'planner-agent' },
  },
  {
    id: 'filesystem-mcp',
    name: 'Filesystem MCP',
    description: 'MCP server for local filesystem browsing and reads.',
    kind: 'mcp',
    toolCompatibility: ['codex', 'gemini'],
    verified: false,
    sourceKind: 'registry',
    security: { env: false, network: false, fs: true },
    configuration: { readOnly: true },
    manifest: { type: 'mcp', id: 'filesystem-mcp', merge_target: 'mcpServers.filesystem' },
  },
];

export function normalizeToolList(value: string): string[] {
  return value
    .split(',')
    .map((entry) => entry.trim().toLowerCase())
    .filter(Boolean)
    .filter((entry, index, entries) => entries.indexOf(entry) === index);
}

export function kindLabel(kind: CatalogKind): string {
  if (kind === 'mcp') {
    return 'MCP';
  }
  return kind.charAt(0).toUpperCase() + kind.slice(1);
}

export function itemKey(kind: CatalogKind, id: string): string {
  return `${kind}:${id}`;
}

export function readStoredStringArray(key: string): string[] {
  if (typeof window === 'undefined') {
    return [];
  }

  try {
    const raw = window.localStorage.getItem(key);
    if (!raw) {
      return [];
    }

    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      return [];
    }

    return parsed.filter((entry): entry is string => typeof entry === 'string');
  } catch {
    return [];
  }
}

export function writeStoredStringArray(key: string, values: string[]): void {
  if (typeof window === 'undefined') {
    return;
  }

  window.localStorage.setItem(key, JSON.stringify(values));
}

export function readStoredCatalog(): CatalogItem[] {
  if (typeof window === 'undefined') {
    return [];
  }

  try {
    const raw = window.localStorage.getItem(CUSTOM_CATALOG_STORAGE_KEY);
    if (!raw) {
      return [];
    }

    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      return [];
    }

    return parsed
      .filter((value): value is Record<string, unknown> => typeof value === 'object' && value !== null)
      .map((value) => {
        const kind = value.kind === 'agent' || value.kind === 'mcp' ? value.kind : 'skill';
        return {
          id: typeof value.id === 'string' && value.id.trim().length > 0 ? value.id.trim() : 'custom-item',
          name:
            typeof value.name === 'string' && value.name.trim().length > 0
              ? value.name.trim()
              : 'Custom Package',
          description: typeof value.description === 'string' ? value.description : '',
          kind,
          toolCompatibility: Array.isArray(value.toolCompatibility)
            ? value.toolCompatibility.filter((entry): entry is string => typeof entry === 'string')
            : [],
          verified: Boolean(value.verified),
          mandatory: Boolean(value.mandatory),
          sourceKind:
            value.sourceKind === 'builtin' || value.sourceKind === 'registry' ? value.sourceKind : 'remote',
          sourceUrl: typeof value.sourceUrl === 'string' ? value.sourceUrl : undefined,
          security: {
            env: Boolean((value.security as Record<string, unknown> | undefined)?.env),
            network: Boolean((value.security as Record<string, unknown> | undefined)?.network),
            fs: Boolean((value.security as Record<string, unknown> | undefined)?.fs),
          },
          configuration:
            typeof value.configuration === 'object' && value.configuration !== null
              ? (value.configuration as Record<string, unknown>)
              : {},
          manifest:
            typeof value.manifest === 'object' && value.manifest !== null
              ? (value.manifest as Record<string, unknown>)
              : { type: kind, id: value.id },
        } satisfies CatalogItem;
      });
  } catch {
    return [];
  }
}

export function writeStoredCatalog(items: CatalogItem[]): void {
  if (typeof window === 'undefined') {
    return;
  }

  window.localStorage.setItem(CUSTOM_CATALOG_STORAGE_KEY, JSON.stringify(items));
}

export function toInstallDraft(item: CatalogItem): InstallDraft {
  return {
    id: item.id,
    name: item.name,
    description: item.description,
    kind: item.kind,
    toolCompatibilityText: item.toolCompatibility.join(', '),
    verified: item.verified,
    sourceKind: item.sourceKind,
    sourceUrl: item.sourceUrl ?? '',
    security: { ...item.security },
    configurationText: JSON.stringify(item.configuration, null, 2),
  };
}

export function buildManifestFromDraft(draft: InstallDraft): Record<string, unknown> {
  return {
    type: draft.kind,
    id: draft.id,
    name: draft.name,
    description: draft.description,
    source: {
      kind: draft.sourceKind,
      url: draft.sourceUrl || undefined,
    },
    compatibility: {
      tools: normalizeToolList(draft.toolCompatibilityText),
    },
    permissions: {
      env: draft.security.env,
      network: draft.security.network,
      fs: draft.security.fs,
    },
    config: (() => {
      try {
        const parsed = JSON.parse(draft.configurationText) as unknown;
        if (typeof parsed === 'object' && parsed !== null && !Array.isArray(parsed)) {
          return parsed;
        }
      } catch {
        // Keep invalid payload out of generated manifest.
      }
      return {};
    })(),
    policy: {
      data_only: true,
      post_install_scripts: false,
    },
  };
}

export function deriveRiskTags(security: CatalogSecurity): string[] {
  const tags: string[] = [];
  if (security.env) {
    tags.push('Uses environment variables');
  }
  if (security.network) {
    tags.push('Requests network access');
  }
  if (security.fs) {
    tags.push('Reads/writes filesystem paths');
  }
  if (tags.length === 0) {
    tags.push('No elevated runtime permissions declared');
  }
  return tags;
}

export function hasPostInstallScripts(manifest: Record<string, unknown>): boolean {
  const scripts = manifest.scripts;
  if (!scripts || typeof scripts !== 'object') {
    return false;
  }
  if (Array.isArray(scripts) && scripts.length > 0) {
    return true;
  }
  const record = scripts as Record<string, unknown>;
  return Object.keys(record).some((key) => /post[-_]?install/i.test(key));
}

export function upsertCatalogItem(items: CatalogItem[], nextItem: CatalogItem): CatalogItem[] {
  const key = itemKey(nextItem.kind, nextItem.id);
  const index = items.findIndex((entry) => itemKey(entry.kind, entry.id) === key);
  if (index === -1) {
    return [nextItem, ...items];
  }

  const updated = [...items];
  updated[index] = nextItem;
  return updated;
}
