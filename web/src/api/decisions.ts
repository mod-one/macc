import { sendJson } from './client';
import type {ApiPendingApproval} from './models';
export type AutonomyMode = 'low' | 'medium' | 'high' | 'critical' | 'never_ask';
export interface DecisionPolicy { enabled: boolean; human_question_min_risk: AutonomyMode; max_context_bytes: number; max_request_bytes: number; trust_mode: string; mandatory_human_triggers: string[] }
export type InterventionResponse = 'completed' | 'confirm_fact' | 'defer_observation' | 'unable';
export interface Decision {
  id: string; version: number; status: string; effective_risk: string; policy_reason?:string; investigation?:{diagnosis:string;missing_sources?:string[]};
  request: { intervention?:{source_repair_paths?:string[];action:string;inaccessible_reason:string;expected_result:string;verification?:string};artifacts?:ArtifactRef[]; operation_spec?:{adapter:string;target:string;source_revision:string;arguments:Record<string,string>}; task_id: string; kind: string; question: string; context: string; recommendation: string; recommended_option_id: string | null; options: {id: string; label: string;intervention_response?:InterventionResponse; consequences: string[];proposed_patch?:unknown;requires_replan?:boolean}[]; response_fields?: {id:string;label:string;required:boolean;multiline:boolean;choices:string[]}[]; proposed_patch?: unknown; affected_tasks: string[] };
  resolution: null | {text: string; selected_option_id: string | null; actor: string; origin: string; recorded_at: string; advisory: boolean;fields?:Record<string,string>;conditions?:ApprovalCondition[];artifacts?:ArtifactRef[]};
}
export interface DeliveryStatus{task:string;phase:string;current:boolean;delivered:boolean;acknowledged:boolean;decisions:{id:string;version:number}[];compliance:Record<string,{status:string;evidence:string}>|null}
export interface ApprovalTicket{id:string;status:string;approval_url:string|null;message:string;intent:Record<string,unknown>;result?:unknown}
export interface DecisionSnapshot { reference_directions?:{id:string;version:number;plan_id:string;source:string;task:string;question:string;text:string;advisory:boolean;actor:string;origin:string}[]; artifacts?:EvidenceArtifact[]; pending_approvals?:ApiPendingApproval[];operator_identity?:string;analytics?:{decisions:number;reused_answers:number;planning_attempts:number;awaiting_plan_effects:number;median_human_response_seconds:number|null;by_status:Record<string,number>};similar_requests?:{enabled:boolean;authority:boolean;method?:string;suggestions:{left:string;right:string;score_percent:number;action:string}[]}; approval_broker_available?:boolean;approval_tickets?:ApprovalTicket[]; delivery?:DeliveryStatus[];operations?:{operation_id:string;version:number;state:string;evidence:string}[]; enabled: boolean; policy: DecisionPolicy; decisions: Decision[]; plan: null | {id: string; graph_version: number; decision_epoch: number}; projection_error?: string | null; events: {seq: number; kind: string; payload: unknown}[] }
export const getDecisions = () => sendJson<DecisionSnapshot>('/decisions', 'GET');
export const mutateDecisions = (body: Record<string, unknown>) => sendJson<unknown, Record<string, unknown>>('/decisions', 'POST', {}, body);

export const getTaskDecisionContexts = (task: string) => sendJson<unknown>(`/decisions/contexts/${encodeURIComponent(task)}`, 'GET');

export interface ArtifactRef{id:string;digest:string}
export interface EvidenceArtifact extends ArtifactRef{plan_id?:string;label:string;media_type:string;size:number;recorded_at:string;share_content?:boolean;permission_version?:number;data_hex?:string}
export interface ApprovalCondition{id:string;description:string;task_id?:string;source_path?:string;source_digest?:string}
