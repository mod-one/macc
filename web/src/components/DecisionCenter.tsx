import React from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import { getDecisions, mutateDecisions, getTaskDecisionContexts, type Decision, type DecisionSnapshot, type AutonomyMode } from '../api/decisions';

interface Draft { text: string; option: string; advisory: boolean; applyPatch?:boolean; fields?:Record<string,string>; outcome?: 'answer'|'reject'|'request_changes' }
function describePatch(value:unknown):string[]{
 if(!value||typeof value!=='object')return[];const operations=(value as {operations?:unknown}).operations;if(!Array.isArray(operations))return[];
 return operations.map((value:unknown)=>{const o=value as {op?:string;task_id?:string;task?:{id?:string;title?:string};depends_on?:string;replacement_task_id?:string;decision_id?:string;changes?:Record<string,unknown>};switch(o.op){case 'add_task':return `Add ${o.task?.id}: ${o.task?.title??'new task'}`;case 'update_task':return `Update ${o.task_id}: ${Object.keys(o.changes??{}).join(', ')}`;case 'add_dependency':return `${o.task_id} must wait for ${o.depends_on}`;case 'remove_dependency':return `Remove ${o.task_id}'s dependency on ${o.depends_on}`;case 'disable_task':return `Disable ${o.task_id}; preserve its retained work`;case 'supersede_task':return `Replace ${o.task_id} with ${o.replacement_task_id}; preserve history`;case 'bind_decision':return `Apply decision ${o.decision_id} to ${o.task_id}`;default:return o.op??'Unknown graph operation';}});
}
const parseAttestation=(value:string):unknown=>{try{return JSON.parse(value);}catch{return value;}};
const keyOf = (d: Decision) => `${d.id}@${d.version}`;
function readDrafts(): Record<string, Draft> {
  try { const value: unknown = JSON.parse(sessionStorage.getItem('macc.decisionDrafts') ?? '{}'); if (!value || typeof value !== 'object' || Array.isArray(value)) return {}; return Object.fromEntries(Object.entries(value).filter(([,v]) => v && typeof v.text === 'string' && typeof v.option === 'string' && typeof v.advisory === 'boolean' && (v.applyPatch === undefined || typeof v.applyPatch === 'boolean') && (v.fields === undefined || (v.fields && typeof v.fields === 'object' && !Array.isArray(v.fields) && Object.values(v.fields).every(f=>typeof f==='string'))) && (v.outcome===undefined || ['answer','reject','request_changes'].includes(v.outcome)))); } catch { return {}; }
}
function inputBusy() {
  const element = document.activeElement;
  return element instanceof HTMLElement && (element.matches('input, textarea, select, [contenteditable="true"]') || !!document.querySelector('[role="dialog"]'));
}
export function DecisionCenter() {
  const [snapshot, setSnapshot] = React.useState<DecisionSnapshot | null>(null);
  const [open, setOpen] = React.useState(false);
  const [selected, setSelected] = React.useState<string | null>(null);
  const [viewed, setViewed] = React.useState<Decision | null>(null);
  const autoPresented = React.useRef(false);
  const [drafts, setDrafts] = React.useState<Record<string, Draft>>(readDrafts);
  const [deferred, setDeferred] = React.useState<Set<string>>(new Set());
  const [busy, setBusy] = React.useState(false);
  const [message, setMessage] = React.useState('');
  const [error, setError] = React.useState('');
  const [token, setToken] = React.useState('');
  const [attestation,setAttestation]=React.useState('');
  const [operationEvidence,setOperationEvidence]=React.useState('');
  const [challenge,setChallenge]=React.useState('');
  const [guidance, setGuidance] = React.useState('');
  const [patch, setPatch] = React.useState('');
  const [contexts, setContexts] = React.useState('');
  const [preview, setPreview] = React.useState('');
  const [reconcileDigest, setReconcileDigest] = React.useState('');
  const [reviewedPatch, setReviewedPatch] = React.useState('');
  const refresh = React.useCallback(async () => {
    try { const data = await getDecisions(); setSnapshot(data); setError(''); } catch (e) { setError(String(e)); }
  }, []);
  React.useEffect(() => { void refresh(); const timer = window.setInterval(() => void refresh(), 3000); return () => window.clearInterval(timer); }, [refresh]);
  React.useEffect(() => { try { sessionStorage.setItem('macc.decisionDrafts', JSON.stringify(drafts)); } catch { /* Keep drafts in memory. */ } }, [drafts]);
  const pending = snapshot?.decisions.filter(d => d.status === 'pending_human') ?? [];
  const neverAsk = snapshot?.policy.human_question_min_risk === 'never_ask';
  React.useEffect(() => {
    if (neverAsk) { if (autoPresented.current) setOpen(false); autoPresented.current = false; return; }
    const next = pending.find(d => !deferred.has(keyOf(d)));
    if (!open && next && !inputBusy()) { setSelected(next.id); setViewed(next); autoPresented.current = true; setOpen(true); }
  }, [snapshot, deferred]); // A polling update must not replace an answer being typed.
  const fresh = snapshot?.decisions.find(d => d.id === selected);
  const current = open && viewed ? viewed : fresh ?? pending[0] ?? snapshot?.decisions.at(-1);
  const draftKey = current ? keyOf(current) : '';
  const draft = drafts[draftKey] ?? { text: '', option: '', advisory: false };
  const updateDraft = (change: Partial<Draft>) => setDrafts(old => ({ ...old, [draftKey]: { ...draft, ...change } }));
  const submit = async (body: Record<string, unknown>, success: string) => {
    setBusy(true); setError('');
    try { if(attestation.trim() && ['answer','patch_apply','patch_reconcile','configure'].includes(String(body.action))){body.attestation=parseAttestation(attestation);} await mutateDecisions(body); setMessage(success); const data = await getDecisions(); setSnapshot(data); if (body.action === 'answer') setViewed(data.decisions.find(d => d.id === body.id) ?? null); } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  const close = () => { if (current?.status === 'pending_human') setDeferred(old => new Set([...old, keyOf(current)])); setOpen(false); };
  return <Dialog.Root open={open} onOpenChange={value => value ? setOpen(true) : close()}>
    <button type="button" onClick={() => { const next = pending[0] ?? snapshot?.decisions.at(-1); setSelected(next?.id ?? null); setViewed(next ?? null); autoPresented.current = false; setOpen(true); }} aria-label={`Decisions, ${pending.length} pending`} className="rounded border px-2 py-1 text-xs">Decisions {pending.length > 0 ? `(${pending.length})` : ''}</button>
    <Dialog.Portal><Dialog.Overlay className="fixed inset-0 z-50 bg-black/60" /><Dialog.Content className="fixed left-1/2 top-1/2 z-50 flex max-h-[94dvh] w-[96vw] max-w-3xl -translate-x-1/2 -translate-y-1/2 flex-col rounded-xl border border-[var(--border)] bg-[var(--bg-primary)] p-4 text-[var(--text-primary)] shadow-xl">
      <Dialog.Title className="text-lg font-semibold">Decisions and performer questions</Dialog.Title>
      <Dialog.Description className="text-sm text-[var(--text-muted)]">Answer important questions, inspect autonomous choices and see what agents receive. Closing defers; it does not answer.</Dialog.Description>
      <div className="min-h-0 space-y-4 overflow-y-auto py-3">
        {error && <p role="alert" className="text-[var(--error)]">{error}</p>}
        {message && <p role="status">{message}</p>}
        {snapshot?.projection_error && <p role="alert">Dispatch is stopped: {snapshot.projection_error}. Inspect the graph and reconcile before resuming.</p>}
        {!snapshot && <section><label>Remote access token<input type="password" value={token} onChange={e => setToken(e.target.value)} className="block w-full rounded border bg-transparent p-2" /></label><button onClick={() => { sessionStorage.setItem('macc.remoteAccessToken', token); void refresh(); }}>Connect</button></section>}
        {snapshot && <section className="flex flex-wrap items-center gap-3 rounded border p-3">
          <label><input type="checkbox" checked={snapshot.enabled} disabled={busy} onChange={e => void submit({action: 'configure', policy: {...snapshot.policy, enabled: e.target.checked}}, 'Decision policy saved.')} /> Enable durable decisions</label>
          <label>Autonomy <select value={snapshot.policy.human_question_min_risk} disabled={busy} onChange={e => void submit({action: 'configure', policy: {...snapshot.policy, human_question_min_risk: e.target.value as AutonomyMode}}, 'Autonomy policy saved; pending work will be re-evaluated.')} className="rounded border bg-[var(--bg-primary)] p-1">
            {(['low', 'medium', 'high', 'critical', 'never_ask'] as const).map(mode => <option key={mode} value={mode}>{mode === 'never_ask' ? 'Full autonomy — never ask' : `Ask from ${mode} risk`}</option>)}
          </select></label><p className="w-full text-xs">Higher threshold means more autonomy. Full autonomy records delegated agent choices, never human votes. Trust: {snapshot.policy.trust_mode}.</p>{snapshot.policy.trust_mode==='secure_approval'&&<label>Isolated approver attestation JSON<textarea value={attestation} onChange={e=>setAttestation(e.target.value)} className="block w-full rounded border bg-transparent p-2" /></label>}
        </section>}
        {snapshot?.enabled && <>
          <label>Request / decision<select className="block w-full rounded border bg-[var(--bg-primary)] p-2" value={current?.id ?? ''} onChange={e => { setSelected(e.target.value); setViewed(snapshot.decisions.find(d => d.id === e.target.value) ?? null); setMessage(''); }}><option value="" disabled>No decisions yet</option>{snapshot.decisions.map(d => <option key={d.id} value={d.id}>{d.status} · {d.request.task_id} · {d.request.question}</option>)}</select></label>
          {current && fresh && current.version !== fresh.version && <p role="status">This request changed or was answered elsewhere. Your draft is preserved; select it again to review the current version.</p>}
          {neverAsk && <p role="status">Full autonomy is active. Choices are recorded below; no answer is required.</p>}
          {current && <section className="space-y-3">
            <h3 className="font-semibold">{current.request.question}</h3><p className="whitespace-pre-wrap text-sm">{current.request.context}</p>
            <p className="text-xs">{current.request.kind} · Risk: {current.effective_risk} · Task: {current.request.task_id} · Affects: {current.request.affected_tasks.join(', ')} · Version {current.version}</p>
            {current.investigation && <p role="status">{current.investigation.diagnosis}{current.investigation.missing_sources?.length ? ` Missing sources: ${current.investigation.missing_sources.join(', ')}` : ''}</p>}
            {current.policy_reason && <p className="text-sm">Routing: {current.policy_reason}</p>}
            {current.request.operation_spec && <section><h4>Exact external operation requested</h4><p>{current.request.operation_spec.adapter} → {current.request.operation_spec.target}</p><p>Reviewed commit: {current.request.operation_spec.source_revision}</p><pre className="overflow-auto text-xs">{JSON.stringify(current.request.operation_spec.arguments,null,2)}</pre></section>}
            <p className="rounded border p-2 text-sm"><strong>Recommendation:</strong> {current.request.recommendation}</p>
            {!!current.request.proposed_patch && <section><h4>Proposed graph changes — explicit acceptance required</h4><ul className="list-disc pl-5">{describePatch(current.request.proposed_patch).map((line,i)=><li key={i}>{line}</li>)}</ul><details><summary>Exact patch data</summary><pre className="max-h-60 overflow-auto whitespace-pre-wrap text-xs">{JSON.stringify(current.request.proposed_patch,null,2)}</pre></details></section>}
            {current.resolution ? <section><h4>Recorded outcome</h4><p className="whitespace-pre-wrap">{current.resolution.text}</p><p className="text-xs">{current.resolution.origin} · {current.resolution.actor} · {current.resolution.recorded_at}</p></section> : current.status === 'pending_human' && !neverAsk && <>
              {current.request.options.length > 0 && <fieldset><legend>Choose an option or provide a custom answer</legend><label className="block"><input type="radio" name="decision-option" checked={!draft.option} onChange={() => updateDraft({option:'',applyPatch:false})} /> Custom answer — no predefined option or patch</label>{current.request.options.map(option => <label key={option.id} className="block rounded border p-2"><input type="radio" name="decision-option" checked={draft.option === option.id} onChange={() => updateDraft({option: option.id,applyPatch:false})} /> {option.label}{option.id === current.request.recommended_option_id ? ' (recommended)' : ''}<span className="block text-xs">{option.consequences.join('; ')}</span></label>)}</fieldset>}
              <label>Answer / conditions / remarks<textarea rows={5} value={draft.text} onChange={e => updateDraft({text: e.target.value})} className="block w-full rounded border bg-transparent p-2" /></label>
              {current.request.response_fields?.map(field => <label key={field.id}>{field.label}{field.required ? ' *' : ''}{field.choices.length ? <select value={draft.fields?.[field.id] ?? ''} onChange={e => updateDraft({fields:{...draft.fields,[field.id]:e.target.value}})}><option value="">Choose…</option>{field.choices.map(choice=><option key={choice}>{choice}</option>)}</select> : <textarea rows={field.multiline ? 3 : 1} value={draft.fields?.[field.id] ?? ''} onChange={e => updateDraft({fields:{...draft.fields,[field.id]:e.target.value}})} className="block w-full rounded border bg-transparent p-2" />}</label>)}
              {['approval','authorization'].includes(current.request.kind) && !current.request.response_fields?.some(f=>f.id==='evidence') && <label>Approval evidence *<input value={draft.fields?.evidence ?? ''} onChange={e=>updateDraft({fields:{...draft.fields,evidence:e.target.value}})} className="block w-full rounded border bg-transparent p-2" /></label>}
              {!!current.request.proposed_patch && draft.option===current.request.recommended_option_id && <label><input type="checkbox" checked={!!draft.applyPatch} onChange={e=>updateDraft({applyPatch:e.target.checked})} /> Apply the exact displayed graph patch with this answer</label>}
              <label><input type="checkbox" checked={draft.advisory} onChange={e => updateDraft({advisory: e.target.checked,applyPatch:false})} /> Advisory guidance, not a binding requirement</label>
              <label>Response outcome<select value={draft.outcome ?? 'answer'} onChange={e=>updateDraft({outcome:e.target.value as 'answer'|'reject'|'request_changes',applyPatch:false})} className="block rounded border bg-[var(--bg-primary)] p-2"><option value="answer">Answer / accept</option><option value="reject">Reject — keep dependent work stopped</option><option value="request_changes">Request changes — keep dependent work stopped</option></select></label>
              {snapshot.policy.trust_mode==='secure_approval' && <section className="space-y-2 rounded border p-2"><p>A separately authenticated approver must sign this exact answer. MACC never signs on the performer's behalf.</p><button disabled={busy} onClick={async()=>{try{const result=await mutateDecisions({action:'answer',challenge:true,id:current.id,version:current.version,text:draft.text || current.request.options.find(o=>o.id===draft.option)?.label,option:draft.option||null,advisory:draft.advisory,apply_patch:!!draft.applyPatch,fields:draft.fields??{},outcome:draft.outcome??'answer'});setChallenge(JSON.stringify(result,null,2));}catch(e){setError(String(e));}}}>Prepare approver challenge</button><pre className="max-h-40 overflow-auto whitespace-pre-wrap">{challenge}</pre></section>}
              <button type="button" disabled={busy || (!draft.text.trim() && !draft.option)} onClick={() => void submit({action: 'answer', id: current.id, version: current.version, text: draft.text || current.request.options.find(o => o.id === draft.option)?.label, option: draft.option || null, advisory: draft.advisory, apply_patch:!!draft.applyPatch, fields:draft.fields ?? {},outcome:draft.outcome ?? 'answer',attestation:attestation.trim()?parseAttestation(attestation):null}, 'Answer recorded. Affected tasks will receive the updated context; delivery and compliance are tracked separately.')} className="rounded border bg-[var(--accent)] px-3 py-2">{busy ? 'Recording…' : 'Send answer'}</button>
            </>}
            <details><summary>Delivery, acknowledgement and independent compliance</summary><div className="overflow-auto"><table className="w-full text-left text-xs"><thead><tr><th>Task / phase</th><th>Current context</th><th>Delivered</th><th>ACK</th><th>Review evidence</th></tr></thead><tbody>{snapshot.delivery?.filter(r=>r.decisions.some(d=>d.id===current.id)).map(r=><tr key={r.task+':'+r.phase}><td>{r.task} / {r.phase}</td><td>{r.current?'Current':'Refresh required'}</td><td>{r.delivered?'Yes':'Pending'}</td><td>{r.acknowledged?'Received':'Pending'}</td><td>{r.compliance?.[current.id]?.status??'Not independently checked'} {r.compliance?.[current.id]?.evidence}</td></tr>)}</tbody></table></div></details>
            <details><summary>What agents received</summary><button type="button" onClick={async () => { try { setContexts(JSON.stringify(await getTaskDecisionContexts(current.request.task_id), null, 2)); } catch(e) {setError(String(e));} }}>Load task contexts and receipts</button><pre className="max-h-60 overflow-auto whitespace-pre-wrap text-xs">{contexts}</pre></details>
            <details><summary>Decision and context history</summary><pre className="max-h-60 overflow-auto whitespace-pre-wrap text-xs">{JSON.stringify(snapshot.events.filter(e => JSON.stringify(e.payload).includes(current.id)), null, 2)}</pre></details>
          </section>}
          {!!snapshot.operations?.length && <details><summary>External operation outcomes</summary>{snapshot.operations.map(op=><section key={op.operation_id} className="rounded border p-2"><strong>{op.operation_id}: {op.state}</strong><p>{op.evidence}</p>{['unknown','failed'].includes(op.state)&&<><label>Reconciliation evidence for {op.operation_id}<textarea value={operationEvidence} onChange={e=>setOperationEvidence(e.target.value)} className="block w-full rounded border bg-transparent p-2" /></label><button disabled={busy||!operationEvidence.trim()} onClick={()=>void submit({action:'operation_reconcile',operation:op.operation_id,version:op.version,state:'succeeded',evidence:operationEvidence},'External success recorded from your evidence.')}>Record verified success</button><button disabled={busy||!operationEvidence.trim()} onClick={()=>void submit({action:'operation_reconcile',operation:op.operation_id,version:op.version,state:'failed',evidence:operationEvidence},'Final failure recorded; no retry was launched.')}>Record final failure</button></>}{['unknown','failed'].includes(op.state)&&<button disabled={busy} onClick={()=>void submit({action:'operation_probe',operation:op.operation_id,version:op.version},'Trusted probe recorded; inspect the updated outcome.')}>Check external outcome with trusted probe</button>}</section>)}</details>}
          <details><summary>Add a new project direction</summary><label>Instruction<textarea className="block w-full rounded border bg-transparent p-2" rows={3} value={guidance} onChange={e => setGuidance(e.target.value)} /></label><button disabled={busy || !guidance.trim()} onClick={() => void submit({action: 'feedback', text: guidance, task: '*', advisory: false}, 'Project guidance recorded and queued for propagation.')}>Record guidance</button></details>
          <details><summary>Versioned graph patch</summary><label>Patch JSON<textarea className="block w-full rounded border bg-transparent p-2 font-mono text-xs" rows={7} value={patch} onChange={e => { setPatch(e.target.value); setReviewedPatch(''); setReconcileDigest(''); }} /></label><button disabled={busy} onClick={async () => { try { const body: unknown = JSON.parse(patch); const result = await mutateDecisions({action: 'patch_preview', patch: body}); setPreview(JSON.stringify(result, null, 2)); const projection = (result as {projection?: {changed?: boolean; source_digest?: string}}).projection; setReconcileDigest(projection?.changed ? projection.source_digest ?? '' : ''); setReviewedPatch(patch); } catch (e) { setError(String(e)); } }}>Preview impact</button><button disabled={busy || !reviewedPatch || reviewedPatch !== patch} onClick={() => void submit({action: 'patch_apply', patch: JSON.parse(patch) as unknown}, 'Graph patch committed; projection/recovery is durable.')}>Apply reviewed patch</button>{reconcileDigest && <button disabled={busy || reviewedPatch !== patch} onClick={() => void submit({action: 'patch_reconcile', patch: JSON.parse(patch) as unknown, source_digest: reconcileDigest}, 'Inspected external PRD changes retained in the journal; reviewed graph projected.')}>Reconcile inspected PRD with this patch</button>}<pre className="max-h-60 overflow-auto whitespace-pre-wrap text-xs">{preview}</pre></details>
        </>}
      </div>
      <footer className="flex shrink-0 justify-end border-t pt-3"><Dialog.Close className="rounded border px-3 py-2">{current?.status === 'pending_human' && !neverAsk ? 'Decide later' : 'Close'}</Dialog.Close></footer>
    </Dialog.Content></Dialog.Portal>
  </Dialog.Root>;
}
