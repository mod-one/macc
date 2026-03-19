export const API_PREFIX = '/api/v1';

function normalizeApiBaseUrl(value: string | undefined): string | undefined {
  if (!value) {
    return undefined;
  }

  const trimmedValue = value.trim();
  if (trimmedValue.length === 0) {
    return undefined;
  }

  return trimmedValue.replace(/\/+$/, '');
}

export function resolveApiBaseUrl(baseUrl?: string): string | undefined {
  return normalizeApiBaseUrl(baseUrl) ?? normalizeApiBaseUrl(import.meta.env.VITE_API_BASE_URL);
}
