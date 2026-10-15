import { sendJson } from './client';
export interface SessionPolicy { lease: { ttl_seconds: number; heartbeat_seconds: number }; lifecycle: { max_idle_hours: number }; checkpoints: { max_content_bytes: number; rotate_after_uses: number } }
export interface SessionField { path: string; label: string; default: number; min: number; max: number; unit: string; help: string; activation: string }
export interface LogicalSession { id: string; project_id: string; task_id: string; role: string; tool_id: string; provider_session_id: string | null; lineage_id: string; status: string; generation: number; use_count: number; owner_pid: number | null; expires_at: number | null; updated_at: number }
export interface SessionStoreView { project_root: string; backend: string; project_id: string | null; revision: string; policy: SessionPolicy; policy_source: string; fields: SessionField[]; sessions: LogicalSession[]; checkpoints_ready: boolean; checkpoints: TaskCheckpoint[] }
export interface MigrationPlan { direction: string; source_sha256: string; imported: number; quarantined: number; backup: string | null; applied: boolean }
export const getSessions = (signal?: AbortSignal) => sendJson<SessionStoreView>('/sessions/store', 'GET', { signal });
export const savePolicy = (revision: string, policy: SessionPolicy | null) => sendJson<SessionStoreView, { revision: string; policy: SessionPolicy | null }>('/sessions/policy', 'PUT', {}, { revision, policy });
export const previewPolicy = (revision: string, policy: SessionPolicy | null) => sendJson<unknown, { revision: string; policy: SessionPolicy | null }>('/sessions/policy/preview', 'POST', {}, { revision, policy });
export const planAuthority = (direction: 'migrate' | 'rollback' | 'upgrade') => sendJson<MigrationPlan>(`/sessions/authority/${direction}`, 'GET');
export const applyAuthority = (plan: MigrationPlan) => sendJson<MigrationPlan, { apply: boolean; source_sha256: string }>(`/sessions/authority/${plan.direction === 'json_to_sqlite' ? 'migrate' : plan.direction === 'sqlite_checkpoints' ? 'upgrade' : 'rollback'}`, 'POST', {}, { apply: true, source_sha256: plan.source_sha256 });
export const sessionAction = (action: 'close' | 'recover' | 'backup', id?: string) => sendJson<unknown, { action: string; id?: string; confirm: boolean }>('/sessions/action', 'POST', {}, { action, id, confirm: action === 'close' });
export const getSessionEvents = (id: string) => sendJson<unknown[]>(`/sessions/${encodeURIComponent(id)}/events`, 'GET');

export interface TaskCheckpoint { id: string; task_id: string; role: string; tool_id: string; phase: string; reason: string; validation_state: string; created_at: number; content_sha256: string }
export interface ReconciliationPlan { previous: unknown; current: unknown; current_sha256: string; applied: boolean }
export const getCheckpoint = (id: string) => sendJson<unknown>(`/sessions/checkpoints/${encodeURIComponent(id)}`, 'GET');
export const previewReconciliation = (id: string) => sendJson<ReconciliationPlan>(`/sessions/checkpoints/${encodeURIComponent(id)}/reconcile`, 'GET');
export const applyReconciliation = (id: string, expected_sha256: string) => sendJson<unknown, { expected_sha256: string }>(`/sessions/checkpoints/${encodeURIComponent(id)}/reconcile`, 'POST', {}, { expected_sha256 });
