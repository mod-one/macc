import React from 'react';
import type {Decision, DecisionSnapshot, ArtifactRef, ApprovalCondition} from '../api/decisions';
import {ApprovalConditions, DecisionEvidence} from './DecisionEvidence';

export interface DecisionDraft {
  conditions?: ApprovalCondition[];
  artifacts?: ArtifactRef[];
  text: string;
  option: string;
  advisory: boolean;
  applyPatch?: boolean;
  fields?: Record<string, string>;
  outcome?: 'answer' | 'reject' | 'request_changes' | 'investigate';
  effects?: 'auto' | 'no_graph_change' | 'replan';
}

function choiceText(current:Decision,draft:DecisionDraft):string {
  if(['intervention','approval','authorization'].includes(current.request.kind))return '';
  const values=(current.request.response_fields??[]).flatMap(field=>{const value=draft.fields?.[field.id];return value&&field.choices.includes(value)?[[field.label,value]]:[];});
  return values.length===1?values[0][1]:values.map(([label,value])=>`${label}: ${value}`).join('\n');
}
export function responseProblems(current: Decision, draft: DecisionDraft): Record<string, string> {
  const errors: Record<string, string> = {};
  const answering = (draft.outcome ?? 'answer') === 'answer';
  if (answering && current.request.kind === 'intervention') {
    if (draft.fields?.performed !== 'yes') errors.performed = 'Confirm that you performed the action, or choose “I cannot do this”.';
    if (!draft.fields?.result?.trim()) errors.result = 'Describe the actual result or paste the command output. A general note does not fill the requested result.';
    if (draft.advisory) errors.advisory = 'An intervention result must be explicit; turn off advisory guidance.';
  }
  for (const field of current.request.response_fields ?? []) {
    const value = draft.fields?.[field.id] ?? '';
    if (answering && field.required && !value.trim()) errors[field.id] = `Please complete: ${field.label}.`;
    else if (value && field.choices.length && !field.choices.includes(value)) errors[field.id] = `Choose one of the listed values for ${field.label}.`;
  }
  if (answering && ['approval', 'authorization'].includes(current.request.kind) && !draft.fields?.evidence?.trim()) errors.evidence = 'Provide the evidence for this approval.';
  if (!draft.text.trim() && !draft.option && !choiceText(current,draft) && !(current.request.kind === 'intervention' && draft.fields?.result?.trim())) errors.text = 'Write an answer or choose an option.';
  return errors;
}

export function decisionAnswer(current: Decision, draft: DecisionDraft) {
  return {
    action: 'answer', id: current.id, version: current.version,
    text: draft.text.trim() ? draft.text : current.request.kind === 'intervention' ? draft.fields?.result ?? '' : current.request.options.find(o => o.id === draft.option)?.label ?? choiceText(current,draft),
    option: draft.option || null, advisory: draft.advisory, apply_patch: !!draft.applyPatch,
    effects: draft.effects ?? 'auto', conditions: draft.conditions ?? [], artifacts: draft.artifacts ?? [],
    fields: draft.fields ?? {}, outcome: draft.outcome ?? 'answer',
  };
}

interface Props {
  current: Decision; snapshot: DecisionSnapshot; draft: DecisionDraft;
  update: (change: Partial<DecisionDraft>) => void;
  busy: boolean; stale: boolean; selectedPatch: unknown;
  attestation: string; setAttestation: (value: string) => void;
  send: (body: Record<string, unknown>) => Promise<void>;
  challenge: (body: Record<string, unknown>) => Promise<void>;
  run: (intent: Record<string, unknown>, message: string) => Promise<void>;
  onError: (message: string) => void;
}
export function DecisionResponseForm({current, snapshot, draft, update, busy, stale, selectedPatch, attestation, setAttestation, send, challenge, run, onError}: Props) {
  const [errors, setErrors] = React.useState<Record<string, string>>({});
  const ref = React.useRef<HTMLFormElement>(null);
  const intervention = current.request.kind === 'intervention';
  const answering = (draft.outcome ?? 'answer') === 'answer';
  const manualProof = snapshot.policy.trust_mode === 'secure_approval' && !snapshot.approval_broker_available;
  const fields = (current.request.response_fields ?? []).filter(f => !intervention || !['performed', 'result'].includes(f.id));
  const changeField = (id: string, value: string) => {
    update({fields: {...draft.fields, [id]: value}});
    setErrors(old => {const next = {...old}; delete next[id]; return next;});
  };
  const idFor = (name: string) => `decision-response-${name}`;
  const validate = (proof: boolean) => {
    if (busy || stale) return false;
    const problems = responseProblems(current, draft);
    if (proof && manualProof) {
      try { if (!attestation.trim() || typeof JSON.parse(attestation) !== 'object') throw new Error(); }
      catch { problems.__attestation = 'Paste the independent approver attestation for this exact response, or prepare its challenge below.'; }
    }
    setErrors(problems);
    const first = Object.keys(problems)[0];
    if (first) {
      const element = document.getElementById(idFor(first));
      for (let parent = element?.parentElement; parent; parent = parent.parentElement) {
        if (parent instanceof HTMLDetailsElement) parent.open = true;
      }
      element?.focus();
      // Error messages change layout after this event: reveal the field once
      // React has committed them, including above the fixed mobile footer.
      window.requestAnimationFrame?.(() => element?.scrollIntoView?.({block: 'center', inline: 'nearest'}));
      return false;
    }
    return true;
  };
  const fieldClass = 'block w-full rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] p-3 focus:outline-none focus:ring-2 focus:ring-[var(--accent)] aria-[invalid=true]:border-[var(--error)]';
  const issue = (name: string) => errors[name] && <p id={`${idFor(name)}-error`} className="mt-1 text-sm text-[var(--error)]">{errors[name]}</p>;
  const a11y = (name: string) => ({id: idFor(name), 'aria-invalid': !!errors[name], 'aria-describedby': errors[name] ? `${idFor(name)}-error` : undefined});
  return <form id="decision-response" ref={ref} noValidate onSubmit={e => {e.preventDefault(); if (validate(true)) void send(decisionAnswer(current, draft));}} className="space-y-4">
    {Object.keys(errors).length > 0 && <p role="alert" className="rounded-lg border border-[var(--error)] p-3 text-sm">Your response has not been sent. Complete the highlighted fields; your draft is kept.</p>}
    <details open={intervention || draft.outcome && draft.outcome!=='answer' ? true:undefined}><summary className="cursor-pointer text-sm">Other responses: unable to help / request changes</summary><label className="block text-sm font-medium">Response outcome
      <select value={draft.outcome ?? 'answer'} onChange={e => {update({outcome: e.target.value as DecisionDraft['outcome'], applyPatch: false, conditions: []}); setErrors({});}} className={fieldClass}>
        <option value="answer">{intervention ? 'Action completed — report the result' : 'Answer / accept'}</option>
        <option value="reject">{intervention ? 'I cannot do this — keep dependent work stopped' : 'Reject — keep dependent work stopped'}</option>
        <option value="investigate">Ask the performer to investigate further</option>
        <option value="request_changes">Request changes</option>
      </select>
    </label></details>
    {intervention && answering && <section className="space-y-3 rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] p-3">
      <label className="flex items-start gap-2"><input {...a11y('performed')} aria-required type="checkbox" checked={draft.fields?.performed === 'yes'} onChange={e => changeField('performed', e.target.checked ? 'yes' : 'no')} className="mt-1" />I performed the requested action</label>{issue('performed')}
      <label className="block text-sm font-medium" htmlFor={idFor('result')}>Actual observed result / command output <span aria-hidden="true">*</span></label>
      <textarea {...a11y('result')} aria-required rows={3} placeholder="What happened? Include the requested device status, port or command output." value={draft.fields?.result ?? ''} onChange={e => changeField('result', e.target.value)} className={fieldClass} />{issue('result')}
      <p className="text-xs text-[var(--text-secondary)]">Report what you actually observed. The performer will verify it independently. Do not paste passwords or tokens.</p>
    </section>}
    {current.request.options.length > 0 && <fieldset className="space-y-2"><legend className="mb-2 text-sm font-medium">Choose an option or provide a custom answer</legend>
      {current.request.options.map(option => <label key={option.id} className={`flex cursor-pointer items-start gap-3 rounded-lg border p-3 ${draft.option === option.id ? 'border-[var(--accent)] bg-[var(--bg-secondary)]' : 'border-[var(--border)]'}`}>
        <input type="radio" name="decision-option" checked={draft.option === option.id} onChange={() => update({option: option.id, applyPatch: false})} className="mt-1" />
        <span><span className="font-medium">{option.label}</span>{option.id === current.request.recommended_option_id && <span className="ml-2 text-xs text-[var(--text-secondary)]">Recommended</span>}</span>
      </label>)}
      <label className="flex items-center gap-3 rounded-lg border border-[var(--border)] p-3"><input type="radio" name="decision-option" checked={!draft.option} onChange={() => update({option: '', applyPatch: false})} />Custom answer — no predefined option or patch</label>
    </fieldset>}
    {fields.map(field => <div key={field.id}>
      <label htmlFor={field.choices.length>0&&field.choices.length<=6?undefined:idFor(field.id)} className="mb-1 block text-sm font-medium">{field.label}{field.required && answering ? ' *' : ' (optional)'}</label>
      {field.choices.length > 0 && field.choices.length <= 6 ? <div {...a11y(field.id)} role="group" aria-label={field.label} tabIndex={-1} className="flex flex-wrap gap-2 rounded-lg focus:outline-none focus:ring-2 focus:ring-[var(--accent)]">{field.choices.map(choice=><button key={choice} type="button" aria-pressed={draft.fields?.[field.id]===choice} onClick={()=>changeField(field.id,choice)} className={`min-w-20 rounded-lg border px-4 py-2 ${draft.fields?.[field.id]===choice?'border-[var(--accent)] bg-[var(--accent-bg)]':'border-[var(--border)]'}`}>{choice}</button>)}</div> : field.choices.length > 6 ? <select {...a11y(field.id)} aria-required={field.required && answering} value={draft.fields?.[field.id] ?? ''} onChange={e => changeField(field.id, e.target.value)} className={fieldClass}><option value="">Choose…</option>{field.choices.map(choice => <option key={choice}>{choice}</option>)}</select>
        : <textarea {...a11y(field.id)} aria-required={field.required && answering} rows={field.multiline ? 3 : 1} value={draft.fields?.[field.id] ?? ''} onChange={e => changeField(field.id, e.target.value)} className={fieldClass} />}{issue(field.id)}
    </div>)}
    {['approval', 'authorization'].includes(current.request.kind) && !fields.some(f => f.id === 'evidence') && <label className="block">Approval evidence *<textarea {...a11y('evidence')} value={draft.fields?.evidence ?? ''} onChange={e => changeField('evidence', e.target.value)} className={fieldClass} />{issue('evidence')}</label>}
    <label className="block text-sm font-medium" htmlFor={idFor('text')}>{intervention && answering || !!draft.option || !!choiceText(current,draft) ? 'Additional note (optional)' : 'Answer / conditions / remarks'}</label>
    <textarea {...a11y('text')} rows={intervention ? 2 : 3} value={draft.text} onChange={e => {update({text: e.target.value}); setErrors(old => {const next = {...old}; delete next.text; return next;});}} className={fieldClass} />{issue('text')}
    {!!selectedPatch && <label className="flex items-start gap-2 rounded-lg border border-[var(--border)] p-3"><input type="checkbox" checked={!!draft.applyPatch} onChange={e => update({applyPatch: e.target.checked})} className="mt-1" />Apply the exact displayed graph patch with this answer</label>}
    {manualProof && <section className="space-y-2 rounded-lg border p-3">
      <p className="text-sm">An independent approver must sign this exact response.</p>
      <button type="button" disabled={busy || stale} onClick={() => {if (validate(false)) void challenge(decisionAnswer(current, draft));}} className="rounded border px-3 py-2">Prepare approver challenge</button>
      <label className="block">Isolated approver attestation JSON<textarea {...a11y('__attestation')} value={attestation} onChange={e => setAttestation(e.target.value)} className={fieldClass} />{issue('__attestation')}</label>
    </section>}
    <details className="rounded-lg border border-[var(--border)] p-3"><summary className="cursor-pointer text-sm font-medium">Attachments and advanced response options</summary><div className="mt-3 space-y-3">
      <DecisionEvidence value={draft.artifacts ?? []} onChange={artifacts => update({artifacts})} available={snapshot.artifacts ?? []} onError={onError} plan={snapshot.plan?.id ?? ''} run={run} />
      {answering && <ApprovalConditions value={draft.conditions ?? []} onChange={conditions => update({conditions, applyPatch: false})} />}
      <label className="block"><input {...a11y('advisory')} type="checkbox" checked={draft.advisory} onChange={e => update({advisory: e.target.checked, applyPatch: false})} />Advisory guidance, not a binding requirement{issue('advisory')}</label>
      <label className="block">Effect on the task plan<select value={draft.effects ?? 'auto'} onChange={e => update({effects: e.target.value as DecisionDraft['effects'], applyPatch: false})} className={fieldClass}><option value="auto">Prepare required task-plan changes before resuming</option><option value="no_graph_change">Explicitly confirm the current task plan remains valid</option><option value="replan">Prepare a new task plan for this answer</option></select></label>
    </div></details>
  </form>;
}
