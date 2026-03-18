export type ApiErrorCategory =
  | 'Validation'
  | 'Auth'
  | 'Dependency'
  | 'Conflict'
  | 'NotFound'
  | 'Internal';

export interface ApiErrorBody {
  code: string;
  category: ApiErrorCategory;
  message: string;
  context?: Record<string, unknown>;
  cause?: string;
}

export interface ApiErrorEnvelope {
  error: ApiErrorBody;
}

export interface ApiHealthResponse {
  status: 'ok';
}

export interface ApiThrottledToolStatus {
  tool_id: string;
  throttled_until: string;
  consecutive_count: number;
}

export interface ApiFailureReport {
  message: string;
  task_id: string | null;
  phase: string | null;
  source: string;
  blocking: boolean;
  event_type: string | null;
  kind: string;
  suggested_fixes: string[];
}

export interface ApiCoordinatorStatus {
  total: number;
  todo: number;
  active: number;
  blocked: number;
  merged: number;
  paused: boolean;
  pause_reason: string | null;
  pause_task_id: string | null;
  pause_phase: string | null;
  latest_error: string | null;
  failure_report: ApiFailureReport | null;
  throttled_tools?: ApiThrottledToolStatus[];
  effective_max_parallel?: number;
}

export interface ApiSelectedTask {
  id: string;
  title: string;
  tool: string;
  base_branch: string;
}

export interface ApiCoordinatorCommandResult {
  status?: ApiCoordinatorStatus;
  resumed?: boolean;
  aggregated_performer_logs?: number;
  runtime_status?: string;
  exported_events_path?: string;
  removed_worktrees?: number;
  selected_task?: ApiSelectedTask;
}

export type ApiCoordinatorAction =
  | 'run'
  | 'stop'
  | 'resume'
  | 'dispatch'
  | 'advance'
  | 'reconcile'
  | 'cleanup';

export interface ApiEventPayload {
  schema_version: string;
  event_id: string;
  seq: number;
  ts: string;
  source: string;
  type: string;
  status: string;
  [key: string]: unknown;
}
