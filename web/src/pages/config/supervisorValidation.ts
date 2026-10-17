import type { SupervisorConfig } from '../../api/models';

export function supervisorErrors(config?: SupervisorConfig): string[] {
  if (!config) return [];
  const errors: string[] = [];
  for (const [key, value] of Object.entries(config)) {
    if (typeof value === 'number' && (!Number.isSafeInteger(value) || value < 1)) errors.push(`${key} must be a positive integer`);
    if (key.endsWith('_path') && typeof value === 'string' && (!value.trim() || value.split('/').includes('..'))) errors.push(`${key} must name a file without parent traversal`);
  }
  if (config.validation_commands.some(c => !c.trim())) errors.push('Remove or fill empty validation commands');
  return errors;
}

