import { sendJson } from './client';
export type AutonomyMode = 'low' | 'medium' | 'high' | 'critical' | 'never_ask';
export interface DecisionPolicy { enabled: boolean; human_question_min_risk: AutonomyMode; max_context_bytes: number; max_request_bytes: number; trust_mode: string; mandatory_human_triggers: string[] }
export interface Decision {
  id: string; version: number; status: string; effective_risk: string;
  request: { task_id: string; kind: string; question: string; context: string; recommendation: string; recommended_option_id: string | null; options: {id: string; label: string; consequences: string[]}[]; proposed_patch?: unknown; affected_tasks: string[] };
  resolution: null | {text: string; selected_option_id: string | null; actor: string; origin: string; recorded_at: string; advisory: boolean};
}
export interface DecisionSnapshot { enabled: boolean; policy: DecisionPolicy; decisions: Decision[]; plan: null | {id: string; graph_version: number; decision_epoch: number}; projection_error?: string | null; events: {seq: number; kind: string; payload: unknown}[] }
export const getDecisions = () => sendJson<DecisionSnapshot>('/decisions', 'GET');
export const mutateDecisions = (body: Record<string, unknown>) => sendJson<unknown, Record<string, unknown>>('/decisions', 'POST', {}, body);

export const getTaskDecisionContexts = (task: string) => sendJson<unknown>(`/decisions/contexts/${encodeURIComponent(task)}`, 'GET');
