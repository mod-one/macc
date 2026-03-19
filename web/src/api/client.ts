import type {
  ApiCoordinatorAction,
  ApiCoordinatorCommandResult,
  ApiCoordinatorStatus,
  ApiErrorEnvelope,
  ApiHealthResponse,
} from './models';

export const API_PREFIX = '/api/v1';

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
      ...(cause ? { cause } : {}),
    },
  };
}

export function buildUrl(path: string, baseUrl?: string): string {
  if (!baseUrl) {
    return `${API_PREFIX}${path}`;
  }
  return new URL(`${API_PREFIX}${path}`, baseUrl).toString();
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
