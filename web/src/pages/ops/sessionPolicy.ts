import type { SessionPolicy } from '../../api/sessions';
export function fieldValue(policy: SessionPolicy, path: string): number {
  let value: unknown = policy;
  for (const key of path.split('.')) {
    if (typeof value !== 'object' || value === null || !(key in value)) throw new Error(`Missing session policy field: ${path}`);
    value = (value as Record<string, unknown>)[key];
  }
  if (typeof value !== 'number') throw new Error(`Unsupported session policy type: ${path}`);
  return value;
}
export function fieldChange(policy: SessionPolicy, path: string, value: number): SessionPolicy {
  fieldValue(policy, path);
  const next: SessionPolicy = JSON.parse(JSON.stringify(policy));
  const keys = path.split('.');
  let node = next as unknown as Record<string, unknown>;
  for (const key of keys.slice(0, -1)) node = node[key] as Record<string, unknown>;
  node[keys[keys.length - 1]] = value;
  return next;
}
