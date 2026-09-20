import type { ApiCatalogMcpEntry, ApiCatalogSkillEntry } from '../../api/models';

export type CatalogKind = 'skill' | 'mcp';
export type InstalledFilter = 'all' | 'installed' | 'not-installed';

export interface SkillCatalogViewItem {
  kind: 'skill';
  id: string;
  name: string;
  description: string;
  tags: string[];
  tools: string[];
  sourceKind: string;
  sourceUrl: string;
  mandatory: boolean;
  installed: boolean;
}

export interface McpCatalogViewItem {
  kind: 'mcp';
  id: string;
  name: string;
  description: string;
  tags: string[];
  tools: string[];
  sourceKind: string;
  sourceUrl: string;
  mandatory: false;
  installed: boolean;
}

export type CatalogViewItem = SkillCatalogViewItem | McpCatalogViewItem;

export function kindLabel(kind: CatalogKind): string {
  return kind === 'mcp' ? 'MCP' : 'Skill';
}

export function itemKey(kind: CatalogKind, id: string): string {
  return `${kind}:${id}`;
}

export function skillToViewItem(
  entry: ApiCatalogSkillEntry,
  selectedSkills: Set<string>,
  mandatorySkills: Set<string>,
): SkillCatalogViewItem {
  return {
    kind: 'skill',
    id: entry.id,
    name: entry.name,
    description: entry.description,
    tags: entry.tags,
    tools: entry.tools,
    sourceKind: entry.source.kind,
    sourceUrl: entry.source.url,
    mandatory: entry.mandatory || mandatorySkills.has(entry.id),
    installed: selectedSkills.has(entry.id),
  };
}

export function mcpToViewItem(entry: ApiCatalogMcpEntry, selectedMcp: Set<string>): McpCatalogViewItem {
  return {
    kind: 'mcp',
    id: entry.id,
    name: entry.name,
    description: entry.description,
    tags: entry.tags,
    tools: [],
    sourceKind: entry.source.kind,
    sourceUrl: entry.source.url,
    mandatory: false,
    installed: selectedMcp.has(entry.id),
  };
}
