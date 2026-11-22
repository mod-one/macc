import type { ApprovalCondition } from '../api/decisions';

export function validApprovalConditions(value: unknown): value is ApprovalCondition[] {
  return Array.isArray(value) && value.length <= 16 && value.every(condition =>
    condition && typeof condition === 'object' &&
    typeof condition.id === 'string' && typeof condition.description === 'string' &&
    ['task_id', 'source_path', 'source_digest'].every(key =>
      condition[key] == null || typeof condition[key] === 'string'
    )
  );
}
