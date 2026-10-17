import React from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import { getApprovals, postGateDecision } from '../api/client';
import type { ApiApprovalDecision, ApiPendingApproval } from '../api/models';
import { useCoordinatorStore } from '../store';
import { useNotificationStore } from '../stores/notificationStore';
import { cn } from './styles';

/*
 * Human approval dialog.
 *
 * Opens by itself when a `human_approval` gate waits for a decision, explains
 * the request in full, and records the decision through
 * POST /api/v1/coordinator/approvals/:id/decision. Core validates the role,
 * revision, evidence and reason exactly as for `macc coordinator approve`.
 * "Later" puts a gate off until its subject revision changes (per browser tab).
 */

const SNOOZE_STORAGE_KEY = 'macc.approvals.snoozed';

function gateKey(gate: ApiPendingApproval): string {
  return `${gate.task_id}@${gate.subject_revision ?? ''}`;
}

function loadSnoozed(): string[] {
  try {
    const raw = window.sessionStorage.getItem(SNOOZE_STORAGE_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter((v): v is string => typeof v === 'string') : [];
  } catch {
    return [];
  }
}

function saveSnoozed(keys: string[]): void {
  try {
    window.sessionStorage.setItem(SNOOZE_STORAGE_KEY, JSON.stringify(keys));
  } catch {
    // Storage unavailable: snoozing lasts for this page only.
  }
}

function shortRevision(revision: string | null | undefined): string {
  return revision ? revision.slice(0, 12) : 'unknown';
}

const DECISION_LABELS: Record<ApiApprovalDecision, string> = {
  approved: 'Approve',
  rejected: 'Reject',
  changes_requested: 'Request changes',
};

/** Roles still missing first, then the others. */
function orderedRoles(gate: ApiPendingApproval): string[] {
  const roles = [...gate.missing_roles];
  for (const role of gate.required_roles) {
    if (!roles.includes(role)) roles.push(role);
  }
  return roles;
}

interface SectionProps {
  title: string;
  children: React.ReactNode;
  tone?: 'default' | 'danger';
}

function Section({ title, children, tone = 'default' }: SectionProps) {
  return (
    <section className="space-y-1">
      <h3
        className={cn(
          'text-[11px] font-semibold uppercase tracking-wide',
          tone === 'danger' ? 'text-[var(--error)]' : 'text-[var(--text-muted)]',
        )}
      >
        {title}
      </h3>
      <div className="text-sm text-[var(--text-secondary)]">{children}</div>
    </section>
  );
}

export function ApprovalBrief({ gate }: { gate: ApiPendingApproval }) {
  const unblocks = gate.unblocks.length > 0 ? gate.unblocks.join(', ') : 'nothing else waits on this gate';
  return (
    <div className="space-y-4">
      <p className="rounded-md border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-2 text-sm font-semibold text-[var(--text-primary)]">
        {gate.question}
      </p>
      {gate.description && <Section title="Decision">{gate.description}</Section>}
      {(gate.approval_trigger || gate.governance_ref) && (
        <Section title="Why people decide">
          {gate.approval_trigger && <div>Trigger: {gate.approval_trigger}</div>}
          {gate.governance_ref && <div>Decision rights: {gate.governance_ref}</div>}
        </Section>
      )}
      <Section title="What is being approved">
        <div className="font-medium text-[var(--text-primary)]">
          {gate.subject_task}
          {gate.subject_title ? ` — ${gate.subject_title}` : ''}
        </div>
        <div>
          Revision: <span className="font-mono">{gate.subject_revision ?? 'unknown (no commit carries its [macc:task] trailer yet)'}</span>
        </div>
        {gate.subject_objective && <div>Objective: {gate.subject_objective}</div>}
        {gate.subject_result && <div>Expected result: {gate.subject_result}</div>}
      </Section>
      {gate.subject_description && (
        <Section title="Subject description">
          <div className="whitespace-pre-wrap">{gate.subject_description}</div>
        </Section>
      )}
      {gate.subject_acceptance_criteria.length > 0 && (
        <Section title="Acceptance criteria">
          <ul className="list-disc space-y-0.5 pl-5">
            {gate.subject_acceptance_criteria.map((c) => (
              <li key={c}>{c}</li>
            ))}
          </ul>
        </Section>
      )}
      {gate.diff_stat && (
        <Section title="Changes in the revision">
          <pre className="max-h-48 overflow-auto rounded-md bg-black/30 p-2 font-mono text-xs">{gate.diff_stat}</pre>
        </Section>
      )}
      {gate.risks.length > 0 && (
        <Section title="Risks to weigh" tone="danger">
          <ul className="list-disc space-y-0.5 pl-5">
            {gate.risks.map((r) => (
              <li key={r}>{r}</li>
            ))}
          </ul>
        </Section>
      )}
      <Section title="Approvers">
        <div>Progress: {gate.required}</div>
        <div>Rule: {gate.quorum}</div>
        {gate.missing_roles.length > 0 && <div>Still needed: {gate.missing_roles.join(', ')}</div>}
      </Section>
      {gate.decisions.length > 0 && (
        <Section title="Decisions so far">
          <ul className="space-y-0.5">
            {gate.decisions.map((d) => (
              <li key={`${d.actor}-${d.role}-${d.recorded_at}`}>
                {d.decision} by {d.actor} ({d.role}) at {d.recorded_at}
                {d.evidence ? ` — evidence: ${d.evidence}` : ''}
                {d.reason ? ` — reason: ${d.reason}` : ''}
              </li>
            ))}
          </ul>
        </Section>
      )}
      <Section title="Rules">
        <ul className="list-disc space-y-0.5 pl-5">
          <li>
            {gate.evidence_required
              ? `An approval must cite ${gate.evidence_type ?? 'evidence'} evidence (a link or path).`
              : 'No evidence is required with an approval.'}
          </li>
          <li>A rejection or a change request must give a reason.</li>
          {gate.invalidate_on_subject_change && (
            <li>Decisions are bound to this revision: a new revision of {gate.subject_task} cancels them.</li>
          )}
          {gate.expires_after_days != null && <li>Approvals expire after {gate.expires_after_days} day(s).</li>}
        </ul>
      </Section>
      <Section title="Consequences">
        <ul className="list-disc space-y-0.5 pl-5">
          <li>Approve: once the quorum is met, the run continues with {unblocks}.</li>
          <li>
            Reject: the gate fails (E909) and its dependants stay blocked (E905) until {gate.subject_task} is revised.
          </li>
          <li>Request changes: the gate waits for a new revision of {gate.subject_task}.</li>
        </ul>
      </Section>
      {gate.note && <Section title="Status">{gate.note}</Section>}
    </div>
  );
}

interface DecisionFormProps {
  gate: ApiPendingApproval;
  defaultActor: string;
  onRecorded: (message: string) => void;
}

function DecisionForm({ gate, defaultActor, onRecorded }: DecisionFormProps) {
  const roles = orderedRoles(gate);
  const [decision, setDecision] = React.useState<ApiApprovalDecision>('approved');
  const [role, setRole] = React.useState(roles[0] ?? '');
  const [evidence, setEvidence] = React.useState('');
  const [reason, setReason] = React.useState('');
  const [actor, setActor] = React.useState('');
  const [confirming, setConfirming] = React.useState(false);
  const [submitting, setSubmitting] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  // A new gate or revision starts a fresh form.
  React.useEffect(() => {
    setRole(orderedRoles(gate)[0] ?? '');
    setEvidence('');
    setReason('');
    setConfirming(false);
    setError(null);
  }, [gate.task_id, gate.subject_revision]); // eslint-disable-line react-hooks/exhaustive-deps

  const isApproval = decision === 'approved';
  const missing: string[] = [];
  if (!role) missing.push('a role');
  if (isApproval && gate.evidence_required && !evidence.trim()) missing.push('evidence');
  if (!isApproval && !reason.trim()) missing.push('a reason');
  if (isApproval && !gate.subject_revision) missing.push('a known subject revision');

  const submit = async () => {
    setSubmitting(true);
    setError(null);
    try {
      const result = await postGateDecision(gate.task_id, {
        decision,
        role,
        revision: gate.subject_revision ?? undefined,
        evidence: isApproval && evidence.trim() ? evidence.trim() : undefined,
        reason: !isApproval ? reason.trim() : undefined,
        actor: actor.trim() || undefined,
      });
      setConfirming(false);
      onRecorded(result.gate_decision);
    } catch (e) {
      setConfirming(false);
      setError(e instanceof Error ? e.message : 'The decision could not be recorded.');
    } finally {
      setSubmitting(false);
    }
  };

  const fieldClass =
    'w-full rounded-md border border-[var(--border)] bg-[var(--bg-card)] px-3 py-2 text-sm text-[var(--text-primary)] outline-none focus:border-[var(--accent)]';

  return (
    <div className="space-y-3">
      <fieldset className="flex flex-wrap gap-2" aria-label="Decision">
        {(Object.keys(DECISION_LABELS) as ApiApprovalDecision[]).map((d) => (
          <label
            key={d}
            className={cn(
              'cursor-pointer rounded-md border px-3 py-1.5 text-sm',
              decision === d
                ? 'border-[var(--accent)] bg-[var(--accent)]/15 text-[var(--text-primary)]'
                : 'border-[var(--border)] text-[var(--text-secondary)]',
            )}
          >
            <input
              type="radio"
              name="approval-decision"
              value={d}
              checked={decision === d}
              onChange={() => {
                setDecision(d);
                setConfirming(false);
              }}
              className="sr-only"
            />
            {DECISION_LABELS[d]}
          </label>
        ))}
      </fieldset>
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="space-y-1 text-xs text-[var(--text-muted)]">
          <span>Role you decide as</span>
          <select className={fieldClass} value={role} onChange={(e) => setRole(e.target.value)}>
            {roles.map((r) => (
              <option key={r} value={r}>
                {r} {gate.missing_roles.includes(r) ? '(still needed)' : '(already approved)'}
              </option>
            ))}
          </select>
        </label>
        <label className="space-y-1 text-xs text-[var(--text-muted)]">
          <span>Decide as (default: {defaultActor || 'server git identity'})</span>
          <input
            className={fieldClass}
            value={actor}
            onChange={(e) => setActor(e.target.value)}
            placeholder={defaultActor || 'Name <email>'}
          />
        </label>
      </div>
      {isApproval ? (
        <label className="block space-y-1 text-xs text-[var(--text-muted)]">
          <span>
            Evidence ({gate.evidence_type ?? 'optional'}){gate.evidence_required ? ' — required' : ' — optional'}
          </span>
          <input
            className={fieldClass}
            value={evidence}
            onChange={(e) => setEvidence(e.target.value)}
            placeholder="https://… pull request review, ADR path, changelog entry"
          />
        </label>
      ) : (
        <label className="block space-y-1 text-xs text-[var(--text-muted)]">
          <span>Reason (what must change, and why) — required</span>
          <textarea className={cn(fieldClass, 'min-h-20')} value={reason} onChange={(e) => setReason(e.target.value)} />
        </label>
      )}
      {error && (
        <p role="alert" className="text-sm text-[var(--error)]">
          Not recorded: {error}
        </p>
      )}
      {confirming ? (
        <div className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-2 text-sm">
          <span>
            Record <strong>{decision}</strong> on {gate.task_id} as <strong>{role}</strong> for revision{' '}
            <span className="font-mono">{shortRevision(gate.subject_revision)}</span>?
          </span>
          <span className="flex gap-2">
            <button
              type="button"
              className="rounded-md border border-[var(--border)] px-3 py-1.5"
              onClick={() => setConfirming(false)}
              disabled={submitting}
            >
              Cancel
            </button>
            <button
              type="button"
              className="rounded-md bg-[var(--accent)] px-3 py-1.5 font-semibold text-white disabled:opacity-50"
              onClick={() => void submit()}
              disabled={submitting}
            >
              {submitting ? 'Recording…' : 'Confirm'}
            </button>
          </span>
        </div>
      ) : (
        <div className="flex items-center justify-end gap-3">
          {missing.length > 0 && <span className="text-xs text-[var(--text-muted)]">Needs {missing.join(', ')}.</span>}
          <button
            type="button"
            className="rounded-md bg-[var(--accent)] px-4 py-2 text-sm font-semibold text-white disabled:cursor-not-allowed disabled:opacity-50"
            disabled={missing.length > 0}
            onClick={() => setConfirming(true)}
          >
            {DECISION_LABELS[decision]}…
          </button>
        </div>
      )}
    </div>
  );
}

/**
 * Topbar pill + dialog. Mounted once in the Layout so a waiting approval
 * surfaces on every page.
 */
export function ApprovalCenter() {
  const status = useCoordinatorStore((s) => s.status);
  const loadStatus = useCoordinatorStore((s) => s.loadStatus);
  const awaiting = React.useMemo(
    () => (status?.pending_approvals ?? []).filter((g) => g.awaiting_decision),
    [status],
  );
  const [snoozed, setSnoozed] = React.useState<string[]>(() => loadSnoozed());
  const [openId, setOpenId] = React.useState<string | null>(null);
  const [message, setMessage] = React.useState<string | null>(null);
  const [defaultActor, setDefaultActor] = React.useState('');

  const gate = awaiting.find((g) => g.task_id === openId) ?? null;

  // Open by itself for the first gate not put off; close once decided elsewhere.
  React.useEffect(() => {
    if (openId && !awaiting.some((g) => g.task_id === openId)) {
      setOpenId(null);
    }
    if (!openId) {
      const next = awaiting.find((g) => !snoozed.includes(gateKey(g)));
      if (next) setOpenId(next.task_id);
    }
  }, [awaiting, openId, snoozed]);

  React.useEffect(() => {
    if (!gate || defaultActor) return;
    getApprovals()
      .then((r) => setDefaultActor(r.default_actor))
      .catch(() => undefined);
  }, [gate, defaultActor]);

  const later = () => {
    if (gate) {
      const next = Array.from(new Set([...snoozed, gateKey(gate)]));
      setSnoozed(next);
      saveSnoozed(next);
    }
    setOpenId(null);
    setMessage(null);
  };

  const openFirst = () => {
    const first = awaiting[0];
    if (!first) return;
    const next = snoozed.filter((k) => k !== gateKey(first));
    setSnoozed(next);
    saveSnoozed(next);
    setMessage(null);
    setOpenId(first.task_id);
  };

  const showNext = () => {
    if (awaiting.length < 2 || !gate) return;
    const index = awaiting.findIndex((g) => g.task_id === gate.task_id);
    setMessage(null);
    setOpenId(awaiting[(index + 1) % awaiting.length].task_id);
  };

  const addNotification = useNotificationStore((s) => s.addNotification);

  // The dialog closes by itself once the gate no longer awaits a decision, so
  // the outcome is also kept in the notification center.
  const onRecorded = (line: string) => {
    setMessage(line);
    addNotification({ type: 'success', title: 'Approval decision recorded', message: line });
    void loadStatus().catch(() => undefined);
  };

  if (awaiting.length === 0) return null;

  return (
    <>
      <button
        type="button"
        onClick={openFirst}
        className="rounded-full border border-[var(--warning)] px-2 py-0.5 text-[11px] font-semibold text-[var(--warning)] hover:bg-[var(--warning)]/10"
        title="Human approval gates are waiting for a decision"
      >
        {awaiting.length} approval{awaiting.length > 1 ? 's' : ''} waiting
      </button>
      <Dialog.Root open={gate !== null} onOpenChange={(open) => (!open ? later() : undefined)}>
        <Dialog.Portal>
          <Dialog.Overlay className="fixed inset-0 z-40 bg-black/70 backdrop-blur-sm" />
          <Dialog.Content
            aria-describedby="approval-dialog-description"
            className="fixed left-1/2 top-1/2 z-50 flex max-h-[92vh] w-[min(94vw,56rem)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-[var(--radius-card)] border border-[var(--warning)]/60 bg-[var(--bg-secondary)] text-[var(--text-primary)] shadow-2xl outline-none"
          >
            {gate && (
              <>
                <header className="border-b border-[var(--border)] px-5 py-3">
                  <Dialog.Title className="text-base font-semibold">
                    Approval required — {gate.task_id}
                    {gate.title ? ` · ${gate.title}` : ''}
                  </Dialog.Title>
                  <Dialog.Description id="approval-dialog-description" className="text-xs text-[var(--text-muted)]">
                    No performer runs for this gate: the run waits for people to decide.
                    {awaiting.length > 1 ? ` ${awaiting.length} gates are waiting.` : ''}
                  </Dialog.Description>
                </header>
                <div className="flex-1 overflow-y-auto px-5 py-4">
                  <ApprovalBrief gate={gate} />
                </div>
                <footer className="space-y-3 border-t border-[var(--border)] px-5 py-4">
                  {message && (
                    <p role="status" className="text-sm text-[var(--success)]">
                      {message}
                    </p>
                  )}
                  <DecisionForm gate={gate} defaultActor={defaultActor} onRecorded={onRecorded} />
                  <div className="flex justify-between text-xs">
                    <button type="button" className="text-[var(--text-muted)] hover:underline" onClick={later}>
                      Decide later
                    </button>
                    {awaiting.length > 1 && (
                      <button type="button" className="text-[var(--text-muted)] hover:underline" onClick={showNext}>
                        Next gate →
                      </button>
                    )}
                  </div>
                </footer>
              </>
            )}
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </>
  );
}
