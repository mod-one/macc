import { useState } from 'react';
import { NavLink } from 'react-router-dom';
import { useCoordinatorStore } from '../../store';
import { awaitingApprovals, awaitingDecisions, openApproval, useInboxStore } from '../../stores/inboxStore';

export default function Inbox({ section }: { section: 'approvals' | 'decisions' }) {
  const status = useCoordinatorStore(s => s.status);
  const statusError = useCoordinatorStore(s => s.loadError);
  const snapshot = useInboxStore(s => s.decisions);
  const decisionError = useInboxStore(s => s.decisionError);
  const [filter, setFilter] = useState('');
  const approvals = awaitingApprovals(status);
  const decisions = awaitingDecisions(snapshot);
  const isApprovals = section === 'approvals';
  const query = filter.trim().toLowerCase();
  const matches = (fields: (string | null)[]) => fields.some(field => field?.toLowerCase().includes(query));
  const gates = approvals.filter(g => matches([g.task_id, g.title, g.subject_task, g.subject_revision, ...g.missing_roles]));
  const questions = decisions.filter(d => matches([d.id, d.request.task_id, d.request.question, d.status]));
  const error = isApprovals ? statusError : decisionError;
  const loaded = isApprovals ? status !== null : snapshot !== null;
  const empty = isApprovals ? gates.length === 0 : questions.length === 0;
  const tabStyle = ({ isActive }: { isActive: boolean }) =>
    isActive ? 'border-b-2 border-[var(--accent)] pb-2 font-semibold' : 'pb-2 text-[var(--text-secondary)]';
  return (
    <section className="space-y-4 p-4 sm:p-6" aria-label="Inbox">
      <h1 className="text-xl font-semibold">Inbox</h1>
      <nav className="flex gap-5 border-b border-[var(--border)]" aria-label="Inbox sections">
        <NavLink to="/ops/inbox/approvals" className={tabStyle}>Approvals ({approvals.length})</NavLink>
        <NavLink to="/ops/inbox/decisions" className={tabStyle}>Decisions ({decisions.length})</NavLink>
      </nav>
      <label className="block text-sm">
        {isApprovals ? 'Find an approval' : 'Find a decision'}
        <input type="search" value={filter} onChange={e => setFilter(e.target.value)}
          placeholder={isApprovals ? 'Task, commit or missing role' : 'Task or question'}
          className="mt-1 block w-full max-w-lg rounded border border-[var(--border)] bg-transparent p-2" />
      </label>
      {error && <p role="alert">{error}</p>}
      {!loaded && !error && <p role="status">Loading requests…</p>}
      {loaded && empty && <p>{query ? 'No matching requests.' : isApprovals ? 'No approvals awaiting a decision.' : 'No decisions awaiting a response.'}</p>}
      {isApprovals ? (
        <ul className="divide-y divide-[var(--border)]">
          {gates.map(g => (
            <li key={g.task_id} className="space-y-1 py-3">
              <button type="button" className="text-left font-semibold text-[var(--accent)] hover:underline"
                onClick={() => openApproval(g.task_id)}>{g.task_id}{g.title && ` · ${g.title}`}</button>
              <p className="text-sm">{g.subject_task} · Commit <code className="break-all">{g.subject_revision ?? 'Unknown'}</code></p>
              <p className="text-sm">Missing roles · {g.missing_roles.join(', ') || 'None'}</p>
              <p className="text-sm">Review evidence · {g.evidence_required ? g.evidence_type ?? 'Required' : 'Optional'}</p>
            </li>
          ))}
        </ul>
      ) : (
        <ul className="divide-y divide-[var(--border)]">
          {questions.map(d => <li key={d.id} className="py-3">
            <button type="button" className="text-left text-[var(--accent)] hover:underline"
              onClick={() => useInboxStore.setState({ decisionToInspect: d })}>{d.request.task_id} · {d.request.question}</button>
          </li>)}
        </ul>
      )}
    </section>
  );
}
