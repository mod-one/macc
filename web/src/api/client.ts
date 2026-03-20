import type {
  ApiActionResult,
  ApiApplyRequest,
  ApiApplyResponse,
  ApiBackup,
  ApiCoordinatorAction,
  ApiCoordinatorCommandResult,
  ApiCoordinatorStatus,
  ApiConfigResponse,
  ApiConfigUpdateRequest,
  ApiDoctorReport,
  ApiErrorEnvelope,
  ApiHealthResponse,
  ApiLogContent,
  ApiLogFile,
  ApiPrdResponse,
  ApiPrdUpdateRequest,
  ApiPlanRequest,
  ApiPlanResponse,
  ApiRegistryTask,
  ApiRegistryTaskAction,
  ApiRestoreRequest,
  ApiWorktree,
  ApiWorktreeCreateRequest,
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

async function sendJson<TResponse, TBody = undefined>(
  path: string,
  method: string,
  options: ApiQueryOptions = {},
  body?: TBody,
): Promise<TResponse> {
  const headers: HeadersInit = {
    Accept: 'application/json',
  };

  if (body !== undefined) {
    headers['Content-Type'] = 'application/json';
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

export async function postCoordinatorAction(
  action: ApiCoordinatorAction,
  options: ApiRequestOptions = {},
): Promise<ApiCoordinatorCommandResult> {
  return requestJson<ApiCoordinatorCommandResult>(
    `/coordinator/${action}`,
    {
      method: 'POST',
      signal: options.signal,
    },
    options.baseUrl,
  );
}

export async function getConfig(
  options: ApiRequestOptions = {},
): Promise<ApiConfigResponse> {
  return sendJson<ApiConfigResponse>('/config', 'GET', options);
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
): Promise<ApiActionResult> {
  return sendJson<ApiActionResult, ApiRestoreRequest>(
    `/backups/${encodeURIComponent(id)}/restore`,
    'POST',
    options,
    request,
  );
}
