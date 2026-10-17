/** Compact elapsed time since an RFC 3339 instant: 45s, 12m, 1h05m. */
export function elapsedLabel(since: string | null, now: Date = new Date()): string {
  if (!since) return '-';
  const start = Date.parse(since);
  if (Number.isNaN(start)) return '-';
  const seconds = Math.max(0, Math.floor((now.getTime() - start) / 1000));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h${String(Math.floor((seconds % 3600) / 60)).padStart(2, '0')}m`;
}
