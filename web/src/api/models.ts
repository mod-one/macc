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

export type ApiEventStreamName = 'coordinator_event' | 'heartbeat';

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

export interface ApiEventStreamMessage {
  stream: ApiEventStreamName;
  eventId: string | null;
  receivedAt: string;
  payload: ApiEventPayload;
}

export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

export type ApiWebAssetsMode = 'dist' | 'embedded';

export type ApiScope = 'project' | 'user';

export type ApiPlannedOpKind = 'write' | 'merge' | 'delete' | 'mkdir' | 'other';

export type ApiCheckSeverity = 'error' | 'warning';

export type ApiDoctorCheckKind = 'which' | 'path_exists' | 'custom';

export interface ApiConfigResponse {
  version: string | null;
  enabledTools: string[];
  toolConfig: Record<string, JsonValue>;
  toolSettings: Record<string, JsonValue>;
  standardsPath: string | null;
  standardsInline: Record<string, string>;
  selectedSkills: string[];
  selectedAgents: string[];
  selectedMcp: string[];
  quiet: boolean;
  offline: boolean;
  webPort: number | null;
  webAssets: ApiWebAssetsMode | null;
  ralphEnabled: boolean | null;
  ralphIterationsDefault: number | null;
  ralphBranchName: string | null;
  ralphStopOnFailure: boolean | null;
  coordinatorTool: string | null;
  referenceBranch: string | null;
  prdFile: string | null;
  taskRegistryFile: string | null;
  toolPriority: string[];
  maxParallelPerTool: Record<string, number>;
  toolSpecializations: Record<string, string[]>;
  maxDispatch: number | null;
  maxParallel: number | null;
  timeoutSeconds: number | null;
  phaseRunnerMaxAttempts: number | null;
  logFlushLines: number | null;
  logFlushMs: number | null;
  mirrorJsonDebounceMs: number | null;
  staleClaimedSeconds: number | null;
  staleInProgressSeconds: number | null;
  staleChangesRequestedSeconds: number | null;
  staleAction: string | null;
  storageMode: string | null;
  mergeAiFix: boolean | null;
  mergeJobTimeoutSeconds: number | null;
  mergeHookTimeoutSeconds: number | null;
  ghostHeartbeatGraceSeconds: number | null;
  dispatchCooldownSeconds: number | null;
  jsonCompat: boolean | null;
  legacyJsonFallback: boolean | null;
  errorCodeRetryList: string | null;
  errorCodeRetryMax: number | null;
  cutoverGateWindowEvents: number | null;
  cutoverGateMaxBlockedRatio: number | null;
  cutoverGateMaxStaleRatio: number | null;
  rateLimitBackoffBaseSeconds: number | null;
  rateLimitBackoffMaxSeconds: number | null;
  rateLimitFallbackEnabled: boolean | null;
  rateLimitThrottleParallel: boolean | null;
  requirementsDetected: boolean;
  managedEnvironmentWarnings: string[];
}

export interface ApiConfigUpdateRequest {
  version?: string | null;
  enabledTools?: string[];
  toolConfig?: Record<string, JsonValue>;
  toolSettings?: Record<string, JsonValue>;
  standardsPath?: string | null;
  standardsInline?: Record<string, string>;
  selectedSkills?: string[];
  selectedAgents?: string[];
  selectedMcp?: string[];
  quiet?: boolean;
  offline?: boolean;
  webPort?: number | null;
  webAssets?: ApiWebAssetsMode | null;
  ralphEnabled?: boolean | null;
  ralphIterationsDefault?: number | null;
  ralphBranchName?: string | null;
  ralphStopOnFailure?: boolean | null;
  coordinatorTool?: string | null;
  referenceBranch?: string | null;
  prdFile?: string | null;
  taskRegistryFile?: string | null;
  toolPriority?: string[];
  maxParallelPerTool?: Record<string, number>;
  toolSpecializations?: Record<string, string[]>;
  maxDispatch?: number | null;
  maxParallel?: number | null;
  timeoutSeconds?: number | null;
  phaseRunnerMaxAttempts?: number | null;
  logFlushLines?: number | null;
  logFlushMs?: number | null;
  mirrorJsonDebounceMs?: number | null;
  staleClaimedSeconds?: number | null;
  staleInProgressSeconds?: number | null;
  staleChangesRequestedSeconds?: number | null;
  staleAction?: string | null;
  storageMode?: string | null;
  mergeAiFix?: boolean | null;
  mergeJobTimeoutSeconds?: number | null;
  mergeHookTimeoutSeconds?: number | null;
  ghostHeartbeatGraceSeconds?: number | null;
  dispatchCooldownSeconds?: number | null;
  jsonCompat?: boolean | null;
  legacyJsonFallback?: boolean | null;
  errorCodeRetryList?: string | null;
  errorCodeRetryMax?: number | null;
  cutoverGateWindowEvents?: number | null;
  cutoverGateMaxBlockedRatio?: number | null;
  cutoverGateMaxStaleRatio?: number | null;
  rateLimitBackoffBaseSeconds?: number | null;
  rateLimitBackoffMaxSeconds?: number | null;
  rateLimitFallbackEnabled?: boolean | null;
  rateLimitThrottleParallel?: boolean | null;
}

export interface ApiPrdTask {
  id: string;
  title: string | null;
  priority: string | null;
  category: string | null;
  scope: string | null;
  baseBranch: string | null;
  coordinatorTool: string | null;
  description: string | null;
  objective: string | null;
  result: string | null;
  dependencies: string[];
  exclusiveResources: string[];
  steps: string[];
  notes: string | null;
  metadata: Record<string, JsonValue>;
}

export interface ApiPrdResponse {
  tasks: ApiPrdTask[];
  metadata: Record<string, JsonValue>;
}

export interface ApiPrdUpdateRequest {
  tasks: ApiPrdTask[];
  metadata: Record<string, JsonValue>;
}

export interface ApiPlanRequest {
  scope?: ApiScope | null;
  tools?: string[];
  allowUserScope?: boolean | null;
  includeDiff?: boolean | null;
  explain?: boolean | null;
}

export interface ApiPlanSummary {
  totalActions: number;
  filesWrite: number;
  filesMerge: number;
  consentRequired: number;
  backupRequired: number;
  backupPath: string;
}

export interface ApiPlanFile {
  path: string;
  kind: ApiPlannedOpKind;
  scope: ApiScope;
  consentRequired: boolean;
  backupRequired: boolean;
  setExecutable: boolean;
  explain: string | null;
}

export interface ApiPlanDiff {
  path: string;
  diffKind: string;
  diff: string | null;
  diffTruncated: boolean;
}

export interface ApiPlanConsent {
  id: string;
  scope: ApiScope;
  message: string;
  paths: string[];
}

export interface ApiPlanResponse {
  summary: ApiPlanSummary;
  files: ApiPlanFile[];
  diffs: ApiPlanDiff[];
  risks: string[];
  consents: ApiPlanConsent[];
}

export interface ApiApplyRequest {
  scope?: ApiScope | null;
  tools?: string[];
  allowUserScope?: boolean | null;
  dryRun: boolean;
  yes?: boolean | null;
}

export interface ApiApplyResult {
  path: string;
  kind: ApiPlannedOpKind;
  success: boolean;
  message: string | null;
  backupLocation: string | null;
}

export interface ApiApplyResponse {
  dryRun: boolean;
  appliedActions: number;
  changedFiles: number;
  backupLocations: string[];
  results: ApiApplyResult[];
  warnings: string[];
}

export interface ApiWorktree {
  id: string;
  slug: string | null;
  branch: string | null;
  tool: string | null;
  status: string | null;
  path: string;
  baseBranch: string | null;
  head: string | null;
  scope: string | null;
  feature: string | null;
  locked: boolean;
  prunable: boolean;
  sessionLabel: string | null;
}

export interface ApiWorktreeCreateRequest {
  slug: string;
  tool: string;
  count: number;
  base: string;
  scope?: string | null;
  feature?: string | null;
  skipApply?: boolean | null;
  allowUserScope?: boolean | null;
}

export interface ApiRegistryTaskWorktree {
  worktreePath: string | null;
  branch: string | null;
  baseBranch: string | null;
  lastCommit: string | null;
  sessionId: string | null;
}

export interface ApiRegistryEvent {
  eventId: string | null;
  eventType: string;
  ts: string | null;
  status: string | null;
  severity: string | null;
  message: string | null;
}

export interface ApiRegistryTask {
  id: string;
  title: string | null;
  state: string;
  tool: string | null;
  attempts: number | null;
  heartbeat: string | null;
  delayedUntil: string | null;
  currentPhase: string | null;
  lastError: string | null;
  lastErrorCode: string | null;
  assignee: JsonValue | null;
  worktree: ApiRegistryTaskWorktree | null;
  events: ApiRegistryEvent[];
  updatedAt: string | null;
}

export type ApiRegistryTaskAction =
  | {
      kind: 'requeue';
      justification?: string | null;
    }
  | {
      kind: 'reassign';
      tool: string;
      justification: string;
    };

export interface ApiLogFile {
  path: string;
  size: number;
  modified: string | null;
}

export interface ApiLogContent {
  path: string;
  lines: string[];
  total: number;
}

export interface ApiDoctorIssue {
  name: string;
  toolId: string | null;
  target: string;
  severity: ApiCheckSeverity;
  kind: ApiDoctorCheckKind;
  status: string;
  message: string | null;
}

export interface ApiDoctorReport {
  healthScore: number;
  issuesBySeverity: Record<string, number>;
  issues: ApiDoctorIssue[];
}

export interface ApiBackup {
  id: string;
  timestamp: string;
  files: number;
  path: string;
  userScope: boolean;
}

export interface ApiRestoreRequest {
  backupId?: string | null;
  latest: boolean;
  user: boolean;
  dryRun: boolean;
  yes?: boolean | null;
}

export interface ApiActionResult {
  status?: string;
  message?: string;
  [key: string]: JsonValue | undefined;
}
