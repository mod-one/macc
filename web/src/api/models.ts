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
  retryable: boolean;
  recommended_action?: string;
  context?: Record<string, unknown>;
  cause?: string;
}

export interface ApiErrorEnvelope {
  error: ApiErrorBody;
}

export interface ApiHealthResponse {
  status: 'ok';
  project_root?: string;
}

export type ApiClientKind = 'Tui' | 'Web' | 'Cli';
export type ApiProcessKind = 'Coordinator' | 'Supervisor' | 'WebServer' | 'TerminalSession' | 'Project';
export type ApiOwnershipStatus = 'owner' | 'viewer' | 'unregistered';

export interface ApiClientIdentity {
  client_id: string;
  kind: ApiClientKind;
  connected_at: string;
  last_heartbeat: string;
}

export interface ApiProcessHandle {
  kind: ApiProcessKind;
  project_root: string;
  pid: number | null;
}

export interface ApiTakeoverRequest {
  request_id: string;
  requester: ApiClientIdentity;
  requested_at: string;
}

export interface ApiOwnershipRecord {
  process: ApiProcessHandle;
  owner: ApiClientIdentity | null;
  viewers: ApiClientIdentity[];
  takeover_request: ApiTakeoverRequest | null;
  started_at: string;
}

export interface ApiOwnershipClaimResponse {
  status: ApiOwnershipStatus;
}

export interface ApiTakeoverRequestResponse {
  request_id: string;
}

export interface ApiGitCommit {
  sha: string;
  short_sha: string;
  subject: string;
  author: string;
  timestamp: number;
  parent_shas: string[];
  branch_refs: string[];
  task_id?: string;
}

export interface ApiGitGraphResponse {
  commits: ApiGitCommit[];
  branches: string[];
  head: string;
}

export interface GitCommit {
  sha: string;
  shortSha: string;
  subject: string;
  author: string;
  timestamp: number;
  parentShas: string[];
  branchRefs: string[];
  taskId?: string;
}

export interface GitGraphResponse {
  commits: GitCommit[];
  branches: string[];
  head: string;
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

export interface ApiToolCooldownEntry {
  tool_id: string;
  throttled_until: number;
  remaining_seconds: number;
  backoff_seconds: number;
}

export interface ApiCoordinatorCommandResult {
  status?: ApiCoordinatorStatus;
  resumed?: boolean;
  aggregated_performer_logs?: number;
  runtime_status?: string;
  exported_events_path?: string;
  removed_worktrees?: number;
  selected_task?: ApiSelectedTask;
  tool_cooldowns?: ApiToolCooldownEntry[];
}

export type ApiCoordinatorAction =
  | 'run'
  | 'stop'
  | 'resume'
  | 'dispatch'
  | 'advance'
  | 'reconcile'
  | 'unlock'
  | 'cleanup'
  | 'sync'
  | 'audit-prd';

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
  forceKillGraceSeconds: number | null;
  safetyPolicy?: string | null;
  destructiveActions?: string | null;
  maxReviewCycles?: number | null;
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
  forceKillGraceSeconds?: number | null;
  safetyPolicy?: string | null;
  destructiveActions?: string | null;
  maxReviewCycles?: number | null;
}

export interface ApiToolInstallDescriptor {
  confirmMessage: string;
}

export type ApiToolActionKind =
  | { action: 'openMcp'; targetPointer: string }
  | { action: 'openSkills'; targetPointer: string }
  | { action: 'openAgents'; targetPointer: string }
  | { action: 'custom'; target: string };

export type ApiToolFieldKind =
  | { type: 'bool' }
  | { type: 'enum'; options: string[] }
  | { type: 'text' }
  | { type: 'number' }
  | { type: 'array' }
  | { type: 'action'; action: ApiToolActionKind };

export type ApiToolFieldDefault =
  | { type: 'bool'; value: boolean }
  | { type: 'text'; value: string }
  | { type: 'enum'; value: string }
  | { type: 'number'; value: number }
  | { type: 'array'; value: string[] };

export interface ApiToolField {
  id: string;
  label: string;
  help: string;
  path: string;
  kind: ApiToolFieldKind;
  default?: ApiToolFieldDefault | null;
}

export interface ApiToolDescriptor {
  id: string;
  title: string;
  description: string;
  fields: ApiToolField[];
  install?: ApiToolInstallDescriptor | null;
}

export interface ApiStandardsPreviewRequest {
  standardsPath: string | null;
  standardsInline: Record<string, string>;
}

export interface ApiStandardsPreviewCard {
  id: string;
  title: string;
  content: string;
}

export interface ApiStandardsPreviewResponse {
  cards: ApiStandardsPreviewCard[];
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
  worktrees?: string[];
  allowUserScope?: boolean | null;
  offline?: boolean | null;
  includeDiff?: boolean | null;
  explain?: boolean | null;
}

export type ApiRiskLevel = 'safe' | 'caution' | 'dangerous';

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
  riskLevel: ApiRiskLevel;
  contentPreview: string | null;
  explain: string | null;
}

export interface ApiPlanDiff {
  path: string;
  diffKind: string;
  diff: string | null;
  diffTruncated: boolean;
}

export interface ApiPlanRisk {
  level: ApiRiskLevel;
  message: string;
}

export interface ApiPlanConsent {
  id: string;
  scope: ApiScope;
  classification: ApiRiskLevel;
  message: string;
  paths: string[];
}

export interface ApiPlanResponse {
  summary: ApiPlanSummary;
  files: ApiPlanFile[];
  diffs: ApiPlanDiff[];
  risks: ApiPlanRisk[];
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

export type ApiTerminalType = 'project' | 'worktree';

export interface ApiTerminalCreateRequest {
  terminalType: ApiTerminalType;
  worktreeId?: string | null;
}

export interface ApiTerminalSessionCreated {
  sessionId: string;
  terminalType: ApiTerminalType;
  path: string;
  worktreeId?: string | null;
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
  priority: string | null;
  state: string;
  tool: string | null;
  attempts: number | null;
  heartbeat: string | null;
  delayedUntil: string | null;
  currentPhase: string | null;
  lastError: string | null;
  lastErrorCode: string | null;
  description: string | null;
  objective: string | null;
  result: string | null;
  dependencies: string[];
  exclusiveResources: string[];
  steps: string[];
  notes: string | null;
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
      kind: 'abandon';
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

/** Structured diagnostic finding from extended checks (spec §14.2). */
export interface ApiDiagnosticFinding {
  /** Stable MACC code, e.g. `MACC-GIT-IDENTITY-MISSING`. */
  id: string;
  title: string;
  /** `"ok"` | `"info"` | `"warning"` | `"error"` */
  severity: string;
  /** `"git"` | `"coordinator"` | `"tools"` | `"tasks"` | `"worktrees"` | `"project"` */
  category: string;
  /** Why this matters — shown in the issue card "Why it matters" section. */
  message: string;
  /** Exact command(s) to run. Shown in the "Fix" section of the issue card. */
  recommendedAction?: string;
  fixAvailable: boolean;
}

export interface ApiDoctorReport {
  healthScore: number;
  issuesBySeverity: Record<string, number>;
  issues: ApiDoctorIssue[];
  /** Extended diagnostic findings with stable codes and recommended actions. */
  findings?: ApiDiagnosticFinding[];
  /** True when no blocking (error-severity) findings exist. */
  ready?: boolean;
}

export interface ApiBackup {
  id: string;
  timestamp: string;
  files: number;
  entries: ApiBackupFile[];
  totalSize: number;
  path: string;
  userScope: boolean;
}

export interface ApiBackupFile {
  path: string;
  size: number;
}

export interface ApiRestoreRequest {
  confirmed: boolean;
}

export interface ApiBackupRestoreResult {
  status: string;
  message: string;
  backupId: string;
  restoreBackupId: string;
  restoredFiles: number;
}

export interface ApiActionResult {
  status?: string;
  message?: string;
  [key: string]: JsonValue | undefined;
}

export interface ApiTrustSummary {
  state: 'trusted' | 'caution' | 'risky' | 'blocked';
  local_only: boolean;
  terminal_enabled: boolean;
  user_level_writes: number;
  backups_ready: boolean;
  catalog_pinned: boolean;
  secrets_redacted: boolean;
  server_exposure: string;
  allowed_roots: string[];
  audit_log: string;
}

// ── Shared runtime snapshot (spec §6.3) ──────────────────────────────────────

export interface ApiQueueSummary {
  todo: number;
  ready: number;
  claimed: number;
  in_progress: number;
  testing: number;
  reviewing: number;
  changes_requested: number;
  blocked: number;
  merged: number;
  failed: number;
  total: number;
}

export interface ApiWorkerRuntime {
  id: string;
  worktree_path: string;
  tool: string;
  task_id: string | null;
  branch: string | null;
  phase: string | null;
  runtime_status: string;
  last_heartbeat: string | null;
  retry_count: number;
  delayed_until: string | null;
}

export interface ApiToolThrottleStatus {
  tool: string;
  reason: string;
  error_code: string;
  retryable: boolean;
  delayed_until: string | null;
  backoff_seconds: number;
  effective_parallelism_delta: number;
}

export interface ApiRuntimeEvent {
  ts: string | null;
  event_type: string;
  task_id: string | null;
  phase: string | null;
  status: string | null;
  message: string | null;
}

export interface ApiRuntimeSnapshot {
  generated_at: string;
  project: { name: string; root: string; config_version: string | null };
  coordinator: { running: boolean; paused: boolean; pause_reason: string | null };
  queue: ApiQueueSummary;
  workers: ApiWorkerRuntime[];
  throttled_tools: ApiToolThrottleStatus[];
  recent_events: ApiRuntimeEvent[];
  git: { current_branch: string | null; clean: boolean; worktrees_count: number };
  diagnostics: { issues_count: number; warnings_count: number; critical_count: number };
  active_runs: Array<{ id: string; skill_id: string; status: string; started_at: string }>;
}

export interface ApiSkillItem {
  id: string;
  title: string;
  kind: string;
  risk: string;
  description: string;
}
