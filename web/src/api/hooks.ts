import { sendJson } from './client';
export interface HookDefinition {
  id: string; name: string; description: string; version: string;
  event: 'tool_output' | 'coordinator_evidence' | 'performer_evidence';
  default_enabled: boolean;
  handler: { kind: 'builtin'; name: string } | { kind: 'rules'; steps: Array<{ kind: 'retain_lines'; pattern: string } | { kind: 'replace'; pattern: string; replacement: string }> };
  skills: string[]; source: { repository: string; subpath: string; distribution: string };
}
export interface HookState { hook: HookDefinition; enabled: boolean; manual: boolean | null; required_by: string[]; integration: string }
export interface HooksView { revision: string; hooks: HookState[]; tools: string[] }
export interface HookReview { definition: HookDefinition; sha256: string }
export const getHooks = () => sendJson<HooksView>('/hooks', 'GET');
export const hookAction = (body: Record<string, unknown>) => sendJson<HooksView, Record<string, unknown>>('/hooks/action', 'POST', {}, body);
export const inspectHook = (definition: HookDefinition) => sendJson<HookReview, HookDefinition>('/hooks/inspect', 'POST', {}, definition);
