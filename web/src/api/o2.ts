import { sendJson } from './client';
export type O2Policy = Record<string, Record<string, boolean | number | string>>;
export interface O2Field { path: string; label: string; kind: 'bool' | 'number' | 'enum' | 'string'; min: number; max: number; choices: string[]; default: boolean | number | string; activation: string }
export interface O2View { project_root: string; revision: string; source: string; policy: O2Policy; fields: O2Field[]; sqlite: boolean; ready: boolean }
export interface Evidence { kind: string; reference: string; sha256: string | null; revision: string | null; summary: string }
export interface Memory { id: string; status: string; project_id: string; proposal: { kind: string; title: string; body: string; scope: string; scope_ref: string | null; confidence: number; evidence: Evidence[]; tags: string[]; source_task: string | null; source_revision: string | null }; verified_revision: string | null; staleness_reason: string | null }
export interface MemoryHit { memory: Memory; checksum: string; score: number; rank_reason: string }
export const getO2 = (signal?: AbortSignal) => sendJson<O2View>('/o2', 'GET', { signal });
export const o2Action = <T = unknown>(action: string, fields: Record<string, unknown> = {}, signal?: AbortSignal) => sendJson<T, Record<string, unknown>>('/o2/action', 'POST', { signal }, { action, ...fields });
