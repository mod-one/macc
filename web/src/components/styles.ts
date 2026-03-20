export function cn(...values: Array<string | false | null | undefined>) {
  return values.filter(Boolean).join(' ');
}

export const surfaceClassName =
  'rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] text-[var(--text-primary)] shadow-[var(--shadow-soft)]';

export const interactiveSurfaceClassName =
  'transition-colors duration-150 hover:border-white/15 hover:bg-white/5';
