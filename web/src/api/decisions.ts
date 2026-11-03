import { sendJson } from './client';
export type AutonomyMode = 'low' | 'medium' | 'high' | 'critical' | 'never_ask';
export interface DecisionPolicy { enabled: boolean; human_question_min_risk: AutonomyMode; max_context_bytes: number; max_request_bytes: number; trust_mode: string; mandatory_human_triggers: string[] }
export interface Decision {
  id: string; version: number; status: string; effective_risk: string; policy_reason?:string; investigation?:{diagnosis:string;missing_sources?:string[]};
  request: { operation_spec?:{adapter:string;target:string;source_revision:string;arguments:Record<string,string>}; task_id: string; kind: string; question: string; context: string; recommendation: string; recommended_option_id: string | null; options: {id: string; label: string; consequences: string[]}[]; response_fields?: {id:string;label:string;required:boolean;multiline:boolean;choices:string[]}[]; proposed_patch?: unknown; affected_tasks: string[] };
  resolution: null | {text: string; selected_option_id: string | null; actor: string; origin: string; recorded_at: string; advisory: boolean};
}
export interface DeliveryStatus{task:string;phase:string;current:boolean;delivered:boolean;acknowledged:boolean;decisions:{id:string;version:number}[];compliance:Record<string,{status:string;evidence:string}>|null}
export interface DecisionSnapshot { delivery?:DeliveryStatus[];operations?:{operation_id:string;version:number;state:string;evidence:string}[]; enabled: boolean; policy: DecisionPolicy; decisions: Decision[]; plan: null | {id: string; graph_version: number; decision_epoch: number}; projection_error?: string | null; events: {seq: number; kind: string; payload: unknown}[] }
export const getDecisions = () => sendJson<DecisionSnapshot>('/decisions', 'GET');
export const mutateDecisions = (body: Record<string, unknown>) => sendJson<unknown, Record<string, unknown>>('/decisions', 'POST', {}, body);

export const getTaskDecisionContexts = (task: string) => sendJson<unknown>(`/decisions/contexts/${encodeURIComponent(task)}`, 'GET');
