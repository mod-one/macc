import type { ApiCoordinatorRunSummary } from '../api/models';

export function SupervisorDiagnosis({ note }: { note: NonNullable<ApiCoordinatorRunSummary['supervisor']> }) {
  return (
    <div aria-live="polite" className="mt-2 text-sm">
      <strong>{['Supervisor', note.status, ...(note.task_ids ?? [])].join(' · ')}</strong>
      <p className="whitespace-pre-wrap">{note.diagnosis}</p>
      {note.outcome && <p>{note.outcome}</p>}
      {note.next_action && <p>Supervisor next: {note.next_action}</p>}
    </div>
  );
}

export function PreviousSupervisorDiagnosis({ summary }: { summary?: ApiCoordinatorRunSummary }) {
  if (summary?.supervisor) return null;
  const previous = summary?.recent_runs.slice(1).find((run) => run.supervisor);
  if (!previous?.supervisor) return null;
  return (
    <details className="my-2 text-sm">
      <summary>Previous incident · {previous.run_id} · {previous.supervisor.status}</summary>
      <SupervisorDiagnosis note={previous.supervisor} />
    </details>
  );
}
