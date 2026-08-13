import type {
  ApiActionResult,
  ApiApplyRequest,
  ApiApplyResponse,
  ApiBackup,
  ApiBackupRestoreResult,
  ApiCoordinatorAction,
  ApiCoordinatorCommandResult,
  ApiCoordinatorStatus,
  ApiConfigResponse,
  ApiConfigUpdateRequest,
  ApiDoctorReport,
  ApiErrorEnvelope,
  ApiGitGraphResponse,
  ApiHealthResponse,
  ApiLogContent,
  ApiLogFile,
  ApiOwnershipClaimResponse,
  ApiOwnershipRecord,
  ApiPrdResponse,
  ApiPrdUpdateRequest,
  ApiPlanRequest,
  ApiPlanResponse,
  ApiRegistryTask,
  ApiRegistryTaskAction,
  ApiRestoreRequest,
  ApiTerminalCreateRequest,
  ApiTerminalSessionCreated,
  ApiStandardsPreviewRequest,
  ApiStandardsPreviewResponse,
  ApiToolDescriptor,
  ApiTakeoverRequestResponse,
  ApiWorktree,
  ApiWorktreeCreateRequest,
  ApiTrustSummary,
  ApiRuntimeSnapshot,
  ApiSkillItem,
  ApiCatalogSkillEntry,
  ApiCatalogMcpEntry,
  ApiCatalogSkillStatus,
  ApiSkillLockEntry,
  ApiVerifyFinding,
  GitCommit,
  GitGraphResponse,
} from './models';
import { API_PREFIX, resolveApiBaseUrl } from './config';

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isApiErrorEnvelope(value: unknown): value is ApiErrorEnvelope {
  if (!isRecord(value) || !isRecord(value.error)) {
    return false;
  }
  return (
    typeof value.error.code === 'string' &&
    typeof value.error.category === 'string' &&
    typeof value.error.message === 'string'
  );
}

export class ApiClientError extends Error {
  readonly status: number;
  readonly envelope: ApiErrorEnvelope;

  constructor(status: number, envelope: ApiErrorEnvelope) {
    super(envelope.error.message);
    this.name = 'ApiClientError';
    this.status = status;
    this.envelope = envelope;
  }
}

const WEB_CLIENT_ID_HEADER = 'X-Macc-Client-Id';
const WEB_CLIENT_ID_STORAGE_KEY = 'macc_client_id';
const WEB_OWNERSHIP_MODE_STORAGE_KEY = 'macc.webOwnershipMode';
const OWNERSHIP_MUTATION_PREFIX = '/processes/project/';
const VIEWER_CLEANUP_PATH = '/processes/project/viewer';
const VIEWER_CLEANUP_FLAG = '__maccViewerCleanupRegistered';

function getSessionStorage(): Storage | null {
  if (typeof window === 'undefined') {
    return null;
  }
  return window.sessionStorage;
}

function getLocalStorage(): Storage | null {
  if (
    typeof window === 'undefined' ||
    !window.localStorage ||
    typeof window.localStorage.getItem !== 'function' ||
    typeof window.localStorage.setItem !== 'function'
  ) {
    return null;
  }
  return window.localStorage;
}

function cleanupProjectViewerOnUnload(clientId: string): void {
  void fetch(buildUrl(VIEWER_CLEANUP_PATH), {
    method: 'DELETE',
    keepalive: true,
    headers: {
      Accept: 'application/json',
      [WEB_CLIENT_ID_HEADER]: clientId,
    },
  }).catch(() => undefined);
}

function currentStoredWebClientId(): string | null {
  return getSessionStorage()?.getItem(WEB_CLIENT_ID_STORAGE_KEY) ?? null;
}

function ensureViewerCleanupRegistered(): void {
  if (typeof window === 'undefined') {
    return;
  }
  const viewerCleanupWindow = window as Window & { [VIEWER_CLEANUP_FLAG]?: boolean };
  if (viewerCleanupWindow[VIEWER_CLEANUP_FLAG]) {
    return;
  }
  viewerCleanupWindow[VIEWER_CLEANUP_FLAG] = true;
  window.addEventListener('beforeunload', () => {
    const clientId = currentStoredWebClientId();
    if (clientId) {
      cleanupProjectViewerOnUnload(clientId);
    }
  });
}

function resolveOwnershipClientId(options?: ApiRequestOptions): string {
  return options?.clientId ?? getWebClientId();
}

function ownershipBody<TBody extends Record<string, unknown>>(
  options: ApiRequestOptions,
  body: TBody,
): TBody & { client_id: string } {
  return {
    ...body,
    client_id: resolveOwnershipClientId(options),
  };
}

export function getWebClientId(): string {
  const storage = getSessionStorage();
  if (!storage) {
    return 'web-server-render';
  }
  const existing = storage.getItem(WEB_CLIENT_ID_STORAGE_KEY);
  if (existing) {
    ensureViewerCleanupRegistered();
    return existing;
  }
  const suffix =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  const clientId = suffix;
  storage.setItem(WEB_CLIENT_ID_STORAGE_KEY, clientId);
  ensureViewerCleanupRegistered();
  return clientId;
}

export function setWebOwnershipMode(mode: 'owner' | 'viewer' | 'unknown'): void {
  getLocalStorage()?.setItem(WEB_OWNERSHIP_MODE_STORAGE_KEY, mode);
}

function getWebOwnershipMode(): string | null {
  return getLocalStorage()?.getItem(WEB_OWNERSHIP_MODE_STORAGE_KEY) ?? null;
}

function fallbackErrorEnvelope(message: string, cause?: string): ApiErrorEnvelope {
  return {
    error: {
      code: 'MACC-WEB-0000',
      category: 'Dependency',
      message,
      retryable: true,
      ...(cause ? { cause } : {}),
    },
  };
}

export function buildUrl(path: string, baseUrl?: string): string {
  const resolvedBaseUrl = resolveApiBaseUrl(baseUrl);
  if (!resolvedBaseUrl) {
    return `${API_PREFIX}${path}`;
  }
  return new URL(`${API_PREFIX}${path}`, resolvedBaseUrl).toString();
}

export function buildWebSocketUrl(path: string, baseUrl?: string): string {
  const resolvedBaseUrl = resolveApiBaseUrl(baseUrl);
  const origin = typeof window !== 'undefined' ? window.location.origin : 'http://localhost';
  const url = new URL(
    resolvedBaseUrl ? `${API_PREFIX}${path}` : `${API_PREFIX}${path}`,
    resolvedBaseUrl ?? origin,
  );
  if (url.protocol === 'https:') {
    url.protocol = 'wss:';
  } else if (url.protocol === 'http:') {
    url.protocol = 'ws:';
  }
  return url.toString();
}

async function requestJson<T>(
  path: string,
  init: RequestInit = {},
  baseUrl?: string,
): Promise<T> {
  let response: Response;
  try {
    response = await fetch(buildUrl(path, baseUrl), {
      headers: {
        Accept: 'application/json',
        ...(init.headers ?? {}),
      },
      ...init,
    });
  } catch (error) {
    const cause = error instanceof Error ? error.message : undefined;
    throw new ApiClientError(
      0,
      fallbackErrorEnvelope('Unable to reach web API endpoint.', cause),
    );
  }

  let payload: unknown = null;
  try {
    payload = await response.json();
  } catch {
    payload = null;
  }

  if (!response.ok) {
    const envelope = isApiErrorEnvelope(payload)
      ? payload
      : fallbackErrorEnvelope(
          `API request failed with HTTP ${response.status}.`,
          `status=${response.status}`,
        );
    throw new ApiClientError(response.status, envelope);
  }

  return payload as T;
}

export interface ApiRequestOptions {
  baseUrl?: string;
  signal?: AbortSignal;
  clientId?: string;
}

type QueryValue = string | number | boolean | null | undefined;

interface ApiQueryOptions extends ApiRequestOptions {
  query?: Record<string, QueryValue>;
}

function buildPath(path: string, query?: Record<string, QueryValue>): string {
  if (!query) {
    return path;
  }

  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined && value !== null) {
      params.set(key, String(value));
    }
  }

  const queryString = params.toString();
  return queryString ? `${path}?${queryString}` : path;
}

export async function sendJson<TResponse, TBody = undefined>(
  path: string,
  method: string,
  options: ApiQueryOptions = {},
  body?: TBody,
): Promise<TResponse> {
  if (
    method !== 'GET' &&
    !path.startsWith(OWNERSHIP_MUTATION_PREFIX) &&
    getWebOwnershipMode() === 'viewer'
  ) {
    throw new ApiClientError(403, {
      error: {
        code: 'MACC-WEB-OWNERSHIP',
        category: 'Auth',
        message: 'This Web client is currently a viewer. Request project control before mutating.',
        retryable: false,
        recommended_action: 'Use the Control banner to request ownership, then retry.',
      },
    });
  }

  const headers: HeadersInit = {
    Accept: 'application/json',
  };

  if (body !== undefined) {
    headers['Content-Type'] = 'application/json';
  }
  if (method !== 'GET') {
    headers[WEB_CLIENT_ID_HEADER] = options.clientId ?? getWebClientId();
  }

  return requestJson<TResponse>(
    buildPath(path, options.query),
    {
      method,
      signal: options.signal,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    },
    options.baseUrl,
  );
}

export async function getHealth(
  options: ApiRequestOptions = {},
): Promise<ApiHealthResponse> {
  return requestJson<ApiHealthResponse>(
    '/health',
    {
      method: 'GET',
      signal: options.signal,
    },
    options.baseUrl,
  );
}

export async function getStatus(
  options: ApiRequestOptions = {},
): Promise<ApiCoordinatorStatus> {
  return requestJson<ApiCoordinatorStatus>(
    '/status',
    {
      method: 'GET',
      signal: options.signal,
    },
    options.baseUrl,
  );
}

function mapGitCommit(commit: ApiGitGraphResponse['commits'][number]): GitCommit {
  return {
    sha: commit.sha,
    shortSha: commit.short_sha,
    subject: commit.subject,
    author: commit.author,
    timestamp: commit.timestamp,
    parentShas: commit.parent_shas ?? [],
    branchRefs: commit.branch_refs ?? [],
    taskId: commit.task_id,
  };
}

export async function getGitGraph(
  options: ApiQueryOptions & {
    limit?: number;
    since?: string;
  } = {},
): Promise<GitGraphResponse> {
  const response = await sendJson<ApiGitGraphResponse>('/git/graph', 'GET', {
    ...options,
    query: {
      limit: options.limit,
      since: options.since,
    },
  });

  return {
    commits: response.commits.map(mapGitCommit),
    branches: response.branches,
    head: response.head,
  };
}

export async function postCoordinatorAction(
  action: ApiCoordinatorAction,
  options: ApiRequestOptions = {},
): Promise<ApiCoordinatorCommandResult> {
  return sendJson<ApiCoordinatorCommandResult>(`/coordinator/${action}`, 'POST', options);
}

export async function listProcessOwnership(
  options: ApiRequestOptions = {},
): Promise<ApiOwnershipRecord[]> {
  return sendJson<ApiOwnershipRecord[]>('/processes', 'GET', options);
}

export async function getProjectOwnership(
  options: ApiRequestOptions = {},
): Promise<ApiOwnershipRecord | null> {
  return sendJson<ApiOwnershipRecord | null>('/processes/project/ownership', 'GET', options);
}

export async function claimProjectOwnership(
  options: ApiRequestOptions = {},
): Promise<ApiOwnershipClaimResponse> {
  return sendJson<ApiOwnershipClaimResponse, { pid: null }>(
    '/processes/project/claim',
    'POST',
    options,
    ownershipBody(options, { pid: null }),
  );
}

export async function registerProjectViewer(
  options: ApiRequestOptions = {},
): Promise<void> {
  await sendJson<unknown, { pid: null }>(
    '/processes/project/viewer',
    'POST',
    options,
    ownershipBody(options, { pid: null }),
  );
}

export async function requestProjectTakeover(
  options: ApiRequestOptions = {},
): Promise<ApiTakeoverRequestResponse> {
  return sendJson<ApiTakeoverRequestResponse, { pid: null }>(
    '/processes/project/takeover/request',
    'POST',
    options,
    ownershipBody(options, { pid: null }),
  );
}

export async function respondProjectTakeover(
  requestId: string,
  accept: boolean,
  options: ApiRequestOptions = {},
): Promise<void> {
  await sendJson<unknown, { pid: null; request_id: string; accept: boolean }>(
    '/processes/project/takeover/respond',
    'POST',
    options,
    ownershipBody(options, { pid: null, request_id: requestId, accept }),
  );
}

export async function heartbeatProjectOwnership(
  options: ApiRequestOptions = {},
): Promise<void> {
  await sendJson<unknown, { pid: null }>(
    '/processes/project/heartbeat',
    'POST',
    options,
    ownershipBody(options, { pid: null }),
  );
}

export async function getConfig(
  options: ApiRequestOptions = {},
): Promise<ApiConfigResponse> {
  return sendJson<ApiConfigResponse>('/config', 'GET', options);
}

export async function getToolDescriptors(
  options: ApiRequestOptions = {},
): Promise<ApiToolDescriptor[]> {
  return sendJson<ApiToolDescriptor[]>('/config/tool-descriptors', 'GET', options);
}

export async function updateConfig(
  request: ApiConfigUpdateRequest,
  options: ApiRequestOptions = {},
): Promise<ApiConfigResponse> {
  return sendJson<ApiConfigResponse, ApiConfigUpdateRequest>(
    '/config',
    'PUT',
    options,
    request,
  );
}

export async function getStandardsPreview(
  request: ApiStandardsPreviewRequest,
  options: ApiRequestOptions = {},
): Promise<ApiStandardsPreviewResponse> {
  return sendJson<ApiStandardsPreviewResponse, ApiStandardsPreviewRequest>(
    '/config/standards-preview',
    'POST',
    options,
    request,
  );
}

export async function getPrd(
  options: ApiQueryOptions & { path?: string } = {},
): Promise<ApiPrdResponse> {
  return sendJson<ApiPrdResponse>('/prd', 'GET', {
    ...options,
    query: {
      path: options.path,
    },
  });
}

export async function updatePrd(
  request: ApiPrdUpdateRequest,
  options: ApiQueryOptions & { path?: string } = {},
): Promise<ApiPrdResponse> {
  return sendJson<ApiPrdResponse, ApiPrdUpdateRequest>(
    '/prd',
    'PUT',
    {
      ...options,
      query: {
        path: options.path,
      },
    },
    request,
  );
}

export async function runPlan(
  request: ApiPlanRequest,
  options: ApiRequestOptions = {},
): Promise<ApiPlanResponse> {
  return sendJson<ApiPlanResponse, ApiPlanRequest>(
    '/plan',
    'POST',
    options,
    request,
  );
}

export async function runApply(
  request: ApiApplyRequest,
  options: ApiRequestOptions = {},
): Promise<ApiApplyResponse> {
  return sendJson<ApiApplyResponse, ApiApplyRequest>(
    '/apply',
    'POST',
    options,
    request,
  );
}

export async function createTerminalSession(
  request: ApiTerminalCreateRequest,
  options: ApiRequestOptions = {},
): Promise<ApiTerminalSessionCreated> {
  return sendJson<ApiTerminalSessionCreated, ApiTerminalCreateRequest>(
    '/terminal',
    'POST',
    options,
    request,
  );
}

export async function getWorktrees(
  options: ApiRequestOptions = {},
): Promise<ApiWorktree[]> {
  return sendJson<ApiWorktree[]>('/worktrees', 'GET', options);
}

export async function createWorktree(
  request: ApiWorktreeCreateRequest,
  options: ApiRequestOptions = {},
): Promise<ApiWorktree[]> {
  return sendJson<ApiWorktree[], ApiWorktreeCreateRequest>(
    '/worktrees',
    'POST',
    options,
    request,
  );
}

export async function deleteWorktree(
  id: string,
  request: {
    confirmed: boolean;
    force?: boolean;
  },
  options: ApiRequestOptions = {},
): Promise<ApiActionResult> {
  return sendJson<ApiActionResult, { confirmed: boolean; force?: boolean }>(
    `/worktrees/${encodeURIComponent(id)}`,
    'DELETE',
    options,
    request,
  );
}

export async function runWorktree(
  id: string,
  options: ApiRequestOptions = {},
): Promise<ApiActionResult> {
  return sendJson<ApiActionResult>(`/worktrees/${encodeURIComponent(id)}/run`, 'POST', options);
}

export async function getRegistryTasks(
  options: ApiRequestOptions = {},
): Promise<ApiRegistryTask[]> {
  return sendJson<ApiRegistryTask[]>('/registry/tasks', 'GET', options);
}

export async function requeueTask(
  id: string,
  action: Extract<ApiRegistryTaskAction, { kind: 'requeue' }>,
  options: ApiRequestOptions = {},
): Promise<ApiRegistryTask> {
  return sendJson<ApiRegistryTask, Extract<ApiRegistryTaskAction, { kind: 'requeue' }>>(
    `/registry/tasks/${encodeURIComponent(id)}/requeue`,
    'POST',
    options,
    action,
  );
}

export async function abandonTask(
  id: string,
  action: Extract<ApiRegistryTaskAction, { kind: 'abandon' }>,
  options: ApiRequestOptions = {},
): Promise<ApiRegistryTask> {
  return sendJson<ApiRegistryTask, Extract<ApiRegistryTaskAction, { kind: 'abandon' }>>(
    `/registry/tasks/${encodeURIComponent(id)}/abandon`,
    'POST',
    options,
    action,
  );
}

export async function reassignTask(
  id: string,
  action: Extract<ApiRegistryTaskAction, { kind: 'reassign' }>,
  options: ApiRequestOptions = {},
): Promise<ApiRegistryTask> {
  return sendJson<ApiRegistryTask, Extract<ApiRegistryTaskAction, { kind: 'reassign' }>>(
    `/registry/tasks/${encodeURIComponent(id)}/reassign`,
    'POST',
    options,
    action,
  );
}

export async function getLogs(
  options: ApiRequestOptions = {},
): Promise<ApiLogFile[]> {
  return sendJson<ApiLogFile[]>('/logs', 'GET', options);
}

export async function getLogContent(
  path: string,
  options: ApiQueryOptions & {
    offset?: number;
    limit?: number;
    search?: string;
  } = {},
): Promise<ApiLogContent> {
  return sendJson<ApiLogContent>(`/logs/${encodeURIComponent(path)}`, 'GET', {
    ...options,
    query: {
      offset: options.offset,
      limit: options.limit,
      search: options.search,
    },
  });
}

export async function getDoctorReport(
  options: ApiRequestOptions = {},
): Promise<ApiDoctorReport> {
  return sendJson<ApiDoctorReport>('/doctor', 'GET', options);
}

export async function runDoctorFix(
  options: ApiRequestOptions = {},
): Promise<ApiActionResult> {
  return sendJson<ApiActionResult>('/doctor/fix', 'POST', options);
}

export async function getBackups(
  options: ApiRequestOptions = {},
): Promise<ApiBackup[]> {
  return sendJson<ApiBackup[]>('/backups', 'GET', options);
}

export async function restoreBackup(
  id: string,
  request: ApiRestoreRequest,
  options: ApiRequestOptions = {},
): Promise<ApiBackupRestoreResult> {
  return sendJson<ApiBackupRestoreResult, ApiRestoreRequest>(
    `/backups/${encodeURIComponent(id)}/restore`,
    'POST',
    options,
    request,
  );
}

export async function getToolCooldowns(
  options: ApiRequestOptions = {},
): Promise<ApiCoordinatorCommandResult> {
  return sendJson<ApiCoordinatorCommandResult>('/coordinator/tool-cooldown', 'GET', options);
}

export async function setToolCooldown(
  tool: string,
  durationSeconds: number,
  options: ApiRequestOptions = {},
): Promise<ApiCoordinatorCommandResult> {
  return sendJson<ApiCoordinatorCommandResult, { tool: string; duration_seconds: number }>(
    '/coordinator/tool-cooldown',
    'POST',
    options,
    {
      tool,
      duration_seconds: durationSeconds,
    },
  );
}

export async function clearToolCooldown(
  tool: string,
  options: ApiRequestOptions = {},
): Promise<ApiCoordinatorCommandResult> {
  return sendJson<ApiCoordinatorCommandResult>(
    `/coordinator/tool-cooldown/${encodeURIComponent(tool)}`,
    'DELETE',
    options,
  );
}
export async function getTrust(
  options: ApiRequestOptions = {},
): Promise<ApiTrustSummary> {
  return sendJson<ApiTrustSummary>('/trust', 'GET', options);
}

// ── Shared runtime snapshot (spec §6.7 / §4.21) ──────────────────────────────

export async function getSnapshot(
  options: ApiRequestOptions = {},
): Promise<ApiRuntimeSnapshot> {
  return requestJson<ApiRuntimeSnapshot>(
    '/snapshot',
    { method: 'GET', signal: options.signal },
    options.baseUrl,
  );
}

export async function listSkills(
  options: ApiRequestOptions = {},
): Promise<ApiSkillItem[]> {
  return requestJson<ApiSkillItem[]>(
    '/skills',
    { method: 'GET', signal: options.signal },
    options.baseUrl,
  );
}

// ── Catalog skills lifecycle (spec §15 / §16) ─────────────────────────────────

export async function getCatalogSkillsAvailable(
  options: ApiRequestOptions = {},
): Promise<{ skills: ApiCatalogSkillEntry[] }> {
  return requestJson<{ skills: ApiCatalogSkillEntry[] }>(
    '/catalog/skills/available',
    { method: 'GET', signal: options.signal },
    options.baseUrl,
  );
}

export async function getCatalogMcpAvailable(
  options: ApiRequestOptions = {},
): Promise<{ mcp: ApiCatalogMcpEntry[] }> {
  return requestJson<{ mcp: ApiCatalogMcpEntry[] }>(
    '/catalog/mcp/available',
    { method: 'GET', signal: options.signal },
    options.baseUrl,
  );
}

export async function getCatalogSkillsStatus(
  options: ApiRequestOptions = {},
): Promise<{ skills: ApiCatalogSkillStatus[]; warnings: string[] }> {
  return requestJson<{ skills: ApiCatalogSkillStatus[]; warnings: string[] }>(
    '/catalog/skills/status',
    { method: 'GET', signal: options.signal },
    options.baseUrl,
  );
}

export async function getCatalogSkillsInstalled(
  options: ApiRequestOptions = {},
): Promise<{ skills: ApiSkillLockEntry[]; version: number }> {
  return requestJson<{ skills: ApiSkillLockEntry[]; version: number }>(
    '/catalog/skills/installed',
    { method: 'GET', signal: options.signal },
    options.baseUrl,
  );
}

export async function postCatalogSkillsVerify(
  options: ApiRequestOptions = {},
): Promise<{ ok: boolean; findings: ApiVerifyFinding[]; finding_count: number }> {
  return requestJson<{ ok: boolean; findings: ApiVerifyFinding[]; finding_count: number }>(
    '/catalog/skills/verify',
    { method: 'POST', signal: options.signal },
    options.baseUrl,
  );
}
