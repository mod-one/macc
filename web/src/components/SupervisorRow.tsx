import type { ApiSupervisorLive } from '../api/models';
import { elapsedLabel } from './elapsedLabel';

export interface SupervisorRowProps {
  supervisor: ApiSupervisorLive;
  now?: Date;
}

/**
 * The supervisor's intervention, shown above the workers. It is not a
 * coordinator task, so it carries its own badge, tint and wording.
 */
export function SupervisorRow({ supervisor, now }: SupervisorRowProps) {
  const tone = supervisor.active ? 'var(--info)' : 'var(--warning)';
  const subject =
    supervisor.task_ids.length > 0
      ? `repair ${supervisor.task_ids.join(', ')}`
      : `incident ${supervisor.incident_id}`;

  return (
    <li
      data-testid="supervisor-row"
      className="border-l-2 px-4 py-2.5"
      style={{ borderLeftColor: tone, backgroundColor: 'var(--bg-elevated)' }}
    >
      <div className="flex items-center gap-3">
        <span
          className="shrink-0 rounded-[var(--radius-sm)] px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide"
          style={{ color: tone, border: `1px solid ${tone}` }}
          title="Supervisor intervention, not a coordinator task"
        >
          Supervisor
        </span>
        <span className="min-w-0 flex-1 truncate font-mono text-[13px] text-[var(--text-primary)]">
          {subject}
        </span>
        {supervisor.tool && (
          <span className="shrink-0 text-xs text-[var(--text-secondary)]">{supervisor.tool}</span>
        )}
        <span className="shrink-0 text-[11px] font-medium" style={{ color: tone }}>
          {supervisor.status}
        </span>
        <span className="shrink-0 text-[11px] text-[var(--text-muted)]">
          {elapsedLabel(supervisor.phase_started_at, now)} / {elapsedLabel(supervisor.started_at, now)}
        </span>
      </div>
      {supervisor.activity && (
        <p className="mt-1 truncate text-xs text-[var(--text-secondary)]">
          Doing: {supervisor.activity}
        </p>
      )}
      {supervisor.reason && (
        <p className="mt-1 text-xs" style={{ color: 'var(--error)' }}>
          Reason: {supervisor.reason}
        </p>
      )}
      {supervisor.next_action && (
        <p className="mt-1 text-xs text-[var(--text-primary)]">Next: {supervisor.next_action}</p>
      )}
      {supervisor.findings.length > 0 && (
        <p className="mt-1 truncate text-[11px] text-[var(--text-muted)]">
          {supervisor.findings.length} MACC finding(s) in `macc supervisor report`
        </p>
      )}
    </li>
  );
}
