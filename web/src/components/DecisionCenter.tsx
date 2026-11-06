import React from 'react';
import {DecisionGovernance,ReuseDirection} from './DecisionGovernance';
import {validApprovalConditions} from './DecisionEvidence';
import {DecisionResponseForm, type DecisionDraft as Draft} from './DecisionResponseForm';
import type {ArtifactRef} from '../api/decisions';
import {ApprovalTicketDetails} from './ApprovalTicketDetails';
import {DecisionGateControls} from './DecisionGateControls';
import {DecisionLifecycleControls} from './DecisionLifecycleControls';
import * as Dialog from '@radix-ui/react-dialog';
import { getDecisions, mutateDecisions, getTaskDecisionContexts, type Decision, type DecisionSnapshot, type AutonomyMode } from '../api/decisions';


function describePatch(value:unknown):string[]{
 if(!value||typeof value!=='object')return[];const operations=(value as {operations?:unknown}).operations;if(!Array.isArray(operations))return[];
 return operations.map((value:unknown)=>{const o=value as {op?:string;task_id?:string;task?:{id?:string;title?:string};depends_on?:string;replacement_task_id?:string;decision_id?:string;changes?:Record<string,unknown>};switch(o.op){case 'add_task':return `Add ${o.task?.id}: ${o.task?.title??'new task'}`;case 'update_task':return `Update ${o.task_id}: ${Object.keys(o.changes??{}).join(', ')}`;case 'add_dependency':return `${o.task_id} must wait for ${o.depends_on}`;case 'remove_dependency':return `Remove ${o.task_id}'s dependency on ${o.depends_on}`;case 'disable_task':return `Disable ${o.task_id}; preserve its retained work`;case 'supersede_task':return `Replace ${o.task_id} with ${o.replacement_task_id}; preserve history`;case 'bind_decision':return `Apply decision ${o.decision_id} to ${o.task_id}`;default:return o.op??'Unknown graph operation';}});
}
const parseAttestation=(value:string):unknown=>{try{return JSON.parse(value);}catch{return value;}};
const keyOf = (d: Decision) => `${d.id}@${d.version}`;
function readDrafts(): Record<string, Draft> {
  try { const value: unknown = JSON.parse(sessionStorage.getItem('macc.decisionDrafts') ?? '{}'); if (!value || typeof value !== 'object' || Array.isArray(value)) return {}; return Object.fromEntries(Object.entries(value).filter(([,v]) => v && typeof v.text === 'string' && typeof v.option === 'string' && typeof v.advisory === 'boolean' && (v.applyPatch === undefined || typeof v.applyPatch === 'boolean') && (v.fields === undefined || (v.fields && typeof v.fields === 'object' && !Array.isArray(v.fields) && Object.values(v.fields).every(f=>typeof f==='string'))) && (v.outcome===undefined || ['answer','reject','request_changes','investigate'].includes(v.outcome)) && (v.effects===undefined || ['auto','no_graph_change','replan'].includes(v.effects)) && (v.conditions===undefined || validApprovalConditions(v.conditions)) && (v.artifacts===undefined || Array.isArray(v.artifacts)&&v.artifacts.every((a:unknown)=>a&&typeof a==='object'&&typeof (a as ArtifactRef).id==='string'&&typeof (a as ArtifactRef).digest==='string')))); } catch { return {}; }
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
  const [loadError, setLoadError] = React.useState('');
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
    try { const data = await getDecisions(); setSnapshot(data); setLoadError(''); } catch (e) { setLoadError(String(e)); }
  }, []);
  React.useEffect(() => { void refresh(); const timer = window.setInterval(() => void refresh(), 3000); return () => window.clearInterval(timer); }, [refresh]);
  React.useEffect(() => { try { sessionStorage.setItem('macc.decisionDrafts', JSON.stringify(drafts)); } catch { /* Keep drafts in memory. */ } }, [drafts]);
  const pending = snapshot?.decisions.filter(d => d.status === 'pending_human') ?? [];
  const neverAsk = snapshot?.policy.human_question_min_risk === 'never_ask';
  React.useEffect(() => {
    if (neverAsk) { const authoritative=snapshot?.decisions.find(d=>d.id===viewed?.id); if(autoPresented.current&&authoritative&&authoritative.status!=='pending_human'){setOpen(false);autoPresented.current=false;} return; }
    const next = pending.find(d => !deferred.has(keyOf(d)));
    if (!open && next && !inputBusy()) { setSelected(next.id); setViewed(next); autoPresented.current = true; setOpen(true); }
  }, [snapshot, deferred]); // A polling update must not replace an answer being typed.
  const fresh = snapshot?.decisions.find(d => d.id === selected);
  const current = open && viewed ? viewed : fresh ?? pending[0] ?? snapshot?.decisions.at(-1);
  const draftKey = current ? keyOf(current) : '';
  const draft = drafts[draftKey] ?? { text: '', option: '', advisory: false };
  const updateDraft = (change: Partial<Draft>) => setDrafts(old => ({ ...old, [draftKey]: { ...draft, ...change } }));
  const intentFor=(body:Record<string,unknown>):Record<string,unknown>|null=>{
    const planId=snapshot?.plan?.id;
    switch(body.action){case 'operator':return body.intent as Record<string,unknown>;case 'answer':return {action:'answer',id:body.id,version:body.version,input:{text:body.text,option:body.option,advisory:body.advisory,apply_patch:body.apply_patch,fields:body.fields,outcome:body.outcome,effects:body.effects??'auto',conditions:body.conditions??[],artifacts:body.artifacts??[]}};case 'configure':return {action:'configure',policy:body.policy};case 'feedback':return {action:'feedback',plan_id:planId,task:body.task??'*',text:body.text,advisory:body.advisory??false};case 'operation_reconcile':return {action:'reconcile',plan_id:planId,operation:body.operation,version:body.version,state:body.state,evidence:body.evidence};case 'patch_apply':case 'patch_reconcile':return {action:'graph',patch:body.patch,source_digest:body.source_digest??null};default:return null;}
  };
  const submit = async (body: Record<string, unknown>, success: string) => {
    setBusy(true); setError('');
    try {
      const intent=intentFor(body);let result:unknown;
      if(snapshot?.policy.trust_mode==='secure_approval'&&intent){
        if(snapshot.approval_broker_available&&!attestation.trim()){
          result=await mutateDecisions({action:'approval_begin',intent});setMessage('Independent approval requested. Open its approval page, check the result, then apply the exact reviewed action. Your draft remains unsent.');
        }else{result=await mutateDecisions({action:'operator',intent,attestation:attestation.trim()?parseAttestation(attestation):null});setMessage(success);}
      }else{result=await mutateDecisions(body);setMessage(success);}
      const data=await getDecisions();setSnapshot(data);
      const object=result&&typeof result==='object'?result as Record<string,unknown>:null;const applied=object?.result&&typeof object.result==='object'?object.result as Record<string,unknown>:object;
      if(typeof object?.message==='string'&&typeof object?.status==='string')setMessage(`${object.message} (${object.status})`);
      if(applied?.request&&typeof applied.id==='string'){setSelected(applied.id);setViewed(data.decisions.find(d=>d.id===applied.id)??null);}else if(body.action==='answer'){setViewed(data.decisions.find(d=>d.id===body.id)??null);}
    }catch(e){setError(String(e));}finally{setBusy(false);}

  };
  const runIntent=(intent:Record<string,unknown>,message:string)=>submit({action:'operator',intent,attestation:attestation.trim()?parseAttestation(attestation):null},message);
  const selectedPatch=current?.request.options.find(o=>o.id===draft.option)?.proposed_patch??((draft.option===current?.request.recommended_option_id||!current?.request.recommended_option_id&&!draft.option)?current?.request.proposed_patch:undefined);
  const close = () => { if (current?.status === 'pending_human') setDeferred(old => new Set([...old, keyOf(current)])); setOpen(false); };
  return <Dialog.Root open={open} onOpenChange={value => value ? setOpen(true) : close()}>
    <button type="button" onClick={() => { const next = pending[0] ?? snapshot?.decisions.at(-1); setSelected(next?.id ?? null); setViewed(next ?? null); autoPresented.current = false; setOpen(true); }} aria-label={`Decisions, ${pending.length} pending`} className="rounded border px-2 py-1 text-xs">Decisions {pending.length > 0 ? `(${pending.length})` : ''}</button>
    <Dialog.Portal><Dialog.Overlay className="fixed inset-0 z-50 bg-black/60" /><Dialog.Content onOpenAutoFocus={event => {
      const target = document.getElementById('decision-response')?.querySelector<HTMLElement>('input[type="radio"], [aria-required="true"], textarea');
      if (target) {event.preventDefault(); target.focus({preventScroll:true});}
    }} className="fixed left-1/2 top-1/2 z-50 flex max-h-[94dvh] w-[96vw] max-w-3xl -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden rounded-xl border border-[var(--border)] bg-[var(--bg-primary)] p-4 text-[var(--text-primary)] shadow-xl">
      <header className="max-h-[28dvh] shrink-0 overflow-y-auto border-b border-[var(--border)] pb-3">
        <p className="mb-1 text-xs font-medium uppercase tracking-wide text-[var(--text-secondary)]">{current?.request.kind === 'intervention' ? 'Action needed' : 'Performer question'}{pending.length > 0 ? ` · ${pending.length} pending` : ''}</p>
        <Dialog.Title className="text-lg font-semibold">{current?.request.question ?? 'Decisions and performer questions'}</Dialog.Title>
        <Dialog.Description className="mt-1 text-sm text-[var(--text-secondary)]">{current ? `Task ${current.request.task_id}. Complete the requested details, then send your response. You can decide later without losing your draft.` : 'Questions, recorded responses and project directions.'}</Dialog.Description>
      </header>
      <div className="min-h-0 min-w-0 space-y-4 overflow-y-auto break-words [overflow-wrap:anywhere] py-3">
        {(error || loadError) && <p role="alert" className="rounded-lg border border-[var(--error)] p-3 text-sm text-[var(--error)]">{error || loadError}</p>}
        {message && <p role="status">{message}</p>}
        {snapshot?.projection_error && <p role="alert">Dispatch is stopped: {snapshot.projection_error}. Inspect the graph and reconcile before resuming.</p>}
        {!snapshot && <section><label>Remote access token<input type="password" value={token} onChange={e => setToken(e.target.value)} className="block w-full rounded border bg-transparent p-2" /></label><button onClick={() => { sessionStorage.setItem('macc.remoteAccessToken', token); void refresh(); }}>Connect</button></section>}
        {snapshot?.enabled && <>

          {current && fresh && (current.version !== fresh.version || current.status !== fresh.status) && <p role="status">This request changed or was answered elsewhere. Your draft is preserved. <button type="button" onClick={() => {setViewed(fresh); setSelected(fresh.id); setMessage('');}} className="underline">Review latest request</button></p>}
          {neverAsk && <p role="status">{current?.status==='pending_human'?(autoPresented.current?'Full autonomy was requested. Waiting for its durable resolution before withdrawing this popup; your draft remains unsent.':'Full autonomy is active. The pending request is being reclassified; no answer is required.'):'Full autonomy is active. Choices are recorded below; no answer is required.'}</p>}
          {current && <section className="space-y-3">
            <p className="text-xs text-[var(--text-secondary)]">{current.request.kind} · Risk: {current.effective_risk} · Task: {current.request.task_id} · Affects: {current.request.affected_tasks.join(', ')} · Version {current.version}</p>
            {current.investigation && <p role="status">{current.investigation.diagnosis}{current.investigation.missing_sources?.length ? ` Missing sources: ${current.investigation.missing_sources.join(', ')}` : ''}</p>}
            {current.policy_reason && <p className="text-sm">Routing: {current.policy_reason}</p>}
            {current.request.operation_spec && <section><h4>Exact external operation requested</h4><p>{current.request.operation_spec.adapter} → {current.request.operation_spec.target}</p><p>Reviewed commit: {current.request.operation_spec.source_revision}</p><pre className="overflow-auto text-xs">{JSON.stringify(current.request.operation_spec.arguments,null,2)}</pre></section>}
            {current.request.intervention && <section className="space-y-2 rounded-lg border border-[var(--accent)] bg-[var(--bg-secondary)] p-3"><h3 className="text-sm font-semibold">What you need to do</h3><p className="whitespace-pre-wrap">{current.request.intervention.action}</p><p className="text-sm"><strong>Result to return:</strong> {current.request.intervention.expected_result}</p></section>}
            <details className="rounded-lg border border-[var(--border)] p-3"><summary className="cursor-pointer text-sm font-medium">Context and verification details</summary><div className="mt-3 space-y-2 text-sm"><p className="whitespace-pre-wrap">{current.request.context}</p>{current.request.intervention && <><p>Unavailable capability: {current.request.intervention.inaccessible_reason}</p><p>{current.request.intervention.verification}</p></>}<p><strong>Recommendation:</strong> {current.request.recommendation}</p></div></details>
            {!current.request.intervention && <p className="rounded-lg border border-[var(--border)] p-3 text-sm"><strong>Recommendation:</strong> {current.request.recommendation}</p>}
            {!!(selectedPatch??current.request.proposed_patch) && <section><h4>Proposed graph changes — explicit acceptance required</h4><ul className="list-disc pl-5">{describePatch(selectedPatch??current.request.proposed_patch).map((line,i)=><li key={i}>{line}</li>)}</ul><details><summary>Exact patch data</summary><pre className="max-h-60 overflow-auto whitespace-pre-wrap text-xs">{JSON.stringify(selectedPatch??current.request.proposed_patch,null,2)}</pre></details></section>}
            {current.resolution ? <section><h4>Recorded outcome</h4><p className="whitespace-pre-wrap">{current.resolution.text}</p><pre className="whitespace-pre-wrap text-sm">{current.resolution.fields?.result}</pre>{current.resolution.conditions?.map(c=><p key={c.id}>Required condition: {c.description} {c.task_id?` · Verification task ${c.task_id}`:''} {c.source_path?` · Evidence ${c.source_path} ${c.source_digest}`:''}</p>)}<p className="text-xs">{current.resolution.origin} · {current.resolution.actor} · {current.resolution.recorded_at}</p></section> : current.status === 'pending_human' && !neverAsk && <DecisionResponseForm key={keyOf(current)} current={current} snapshot={snapshot} draft={draft} update={updateDraft} busy={busy}
              stale={!fresh || fresh.version !== current.version || fresh.status !== 'pending_human'} selectedPatch={selectedPatch}
              attestation={attestation} setAttestation={setAttestation} onError={setError} run={runIntent}
              send={body => submit({...body, attestation: attestation.trim() ? parseAttestation(attestation) : null}, 'Response recorded. The performer will receive the updated context and verify the result.')}
              challenge={async body => {try {setChallenge(JSON.stringify(await mutateDecisions({...body, challenge:true}), null, 2));} catch (e) {setError(String(e));}}} />}
            {challenge && <details open><summary>Independent approver challenge</summary><pre className="max-h-40 overflow-auto whitespace-pre-wrap text-xs">{challenge}</pre></details>}
            {current.status==='conditional'&&<p role="status">Your conditional response is saved. It grants no approval until every explicit condition is checked against current evidence.</p>}
            {current.status==='investigating_requested'&&<p role="status">Further investigation requested. A bounded read-only performer invocation will report back before you answer.</p>}
            {current.status==='blocked_external'&&<p role="status">An indispensable external capability is unavailable. Full autonomy does not fabricate an action or a result; only dependent work is blocked.</p>}
            {current.status==='awaiting_effects'&&<p role="status">Your answer is durably saved. Affected implementation waits while the coordinator prepares and validates its task-plan changes. Other tasks can continue.</p>}
            <details className="rounded-lg border border-[var(--border)] p-3"><summary className="cursor-pointer text-sm">Revise, withdraw or manage this request</summary><div className="mt-3"><DecisionLifecycleControls key={keyOf(current)} decision={current} busy={busy} run={runIntent} onError={setError}/></div></details>
            <details><summary>Delivery, acknowledgement and independent compliance</summary><div className="overflow-auto"><table className="w-full text-left text-xs"><thead><tr><th>Task / phase</th><th>Current context</th><th>Delivered</th><th>ACK</th><th>Review evidence</th></tr></thead><tbody>{snapshot.delivery?.filter(r=>r.decisions.some(d=>d.id===current.id)).map(r=><tr key={r.task+':'+r.phase}><td>{r.task} / {r.phase}</td><td>{r.current?'Current':'Refresh required'}</td><td>{r.delivered?'Yes':'Pending'}</td><td>{r.acknowledged?'Received':'Pending'}</td><td>{r.compliance?.[current.id]?.status??'Not independently checked'} {r.compliance?.[current.id]?.evidence}</td></tr>)}</tbody></table></div></details>
            <details><summary>What agents received</summary><button type="button" onClick={async () => { try { setContexts(JSON.stringify(await getTaskDecisionContexts(current.request.task_id), null, 2)); } catch(e) {setError(String(e));} }}>Load task contexts and receipts</button><pre className="max-h-60 overflow-auto whitespace-pre-wrap text-xs">{contexts}</pre></details>
            <details><summary>Decision and context history</summary><pre className="max-h-60 overflow-auto whitespace-pre-wrap text-xs">{JSON.stringify(snapshot.events.filter(e => JSON.stringify(e.payload).includes(current.id)), null, 2)}</pre></details>
          </section>}
        </>}
        {snapshot && <details className="rounded-lg border border-[var(--border)] p-3"><summary className="cursor-pointer text-sm font-medium">All requests and decision management</summary><div className="mt-3 space-y-4">
          <label>Request / decision<select className="block w-full rounded border bg-[var(--bg-primary)] p-2" value={current?.id ?? ''} onChange={e => { setSelected(e.target.value); setViewed(snapshot.decisions.find(d => d.id === e.target.value) ?? null); setMessage(''); }}><option value="" disabled>No decisions yet</option>{snapshot.decisions.map(d => <option key={d.id} value={d.id}>{d.status} · {d.request.task_id} · {d.request.question}</option>)}</select></label>        {snapshot && <section className="flex flex-wrap items-center gap-3 rounded border p-3">
          <label><input type="checkbox" checked={snapshot.enabled} disabled={busy} onChange={e => void submit({action: 'configure', policy: {...snapshot.policy, enabled: e.target.checked}}, 'Decision policy saved.')} /> Enable durable decisions</label>
          <label>Autonomy <select value={snapshot.policy.human_question_min_risk} disabled={busy} onChange={e => void submit({action: 'configure', policy: {...snapshot.policy, human_question_min_risk: e.target.value as AutonomyMode}}, 'Autonomy policy saved; pending work will be re-evaluated.')} className="rounded border bg-[var(--bg-primary)] p-1">
            {(['low', 'medium', 'high', 'critical', 'never_ask'] as const).map(mode => <option key={mode} value={mode}>{mode === 'never_ask' ? 'Full autonomy — never ask' : `Ask from ${mode} risk`}</option>)}
          </select></label><p className="w-full text-xs">Higher threshold means more autonomy. Full autonomy records delegated agent choices, never human votes. Trust: {snapshot.policy.trust_mode}.</p>
        </section>}
        {!!snapshot?.approval_tickets?.length&&<section className="space-y-2 rounded border p-3"><h3>Independent approvals</h3>{snapshot.approval_tickets.filter(t=>!['applied','rejected','stale','expired'].includes(t.status)).map(ticket=><section key={ticket.id} className="space-y-1 border-t p-2"><strong>{String(ticket.intent.action)}: {ticket.status}</strong><p role="status">{ticket.message}</p><ApprovalTicketDetails ticket={ticket}/>{ticket.approval_url&&<a href={ticket.approval_url} target="_blank" rel="noopener noreferrer" className="underline">Open independent approval page</a>}{ticket.status==='pending'&&<button disabled={busy} onClick={()=>void submit({action:'approval_poll',id:ticket.id},'Independent approval status checked. No action is applied until you confirm.')}>Check approval</button>}{['approved','applying'].includes(ticket.status)&&<button disabled={busy} onClick={()=>void submit({action:'approval_apply',id:ticket.id},'Independently approved action recorded.')}>Apply approved action</button>}</section>)}</section>}
          {!neverAsk&&<DecisionGateControls gates={snapshot.pending_approvals??[]} plan={snapshot.plan?.id??''} identity={snapshot.operator_identity??''} busy={busy} run={runIntent}/>}
          <details><summary>Decision diagnostics</summary>{snapshot.analytics&&<dl className="grid grid-cols-2 gap-2"><dt>Recorded decisions</dt><dd>{snapshot.analytics.decisions}</dd><dt>Reused answers</dt><dd>{snapshot.analytics.reused_answers}</dd><dt>Planning invocations</dt><dd>{snapshot.analytics.planning_attempts}</dd><dt>Awaiting task-plan effects</dt><dd>{snapshot.analytics.awaiting_plan_effects}</dd><dt>Median human response (seconds)</dt><dd>{snapshot.analytics.median_human_response_seconds??'No observations'}</dd></dl>}<p>Related-request hints are advisory; they never merge answers or grant consent.</p>{snapshot.similar_requests?.suggestions.map(pair=><p key={pair.left+pair.right}>{pair.left} / {pair.right}: {pair.score_percent}% — {pair.action}</p>)}</details>
          <DecisionGovernance snapshot={snapshot} busy={busy} run={runIntent}/><ReuseDirection snapshot={snapshot} busy={busy} run={runIntent}/>
          {!!snapshot.operations?.length && <details><summary>External operation outcomes</summary>{snapshot.operations.map(op=><section key={op.operation_id} className="rounded border p-2"><strong>{op.operation_id}: {op.state}</strong><p>{op.evidence}</p>{['unknown','failed'].includes(op.state)&&<><label>Reconciliation evidence for {op.operation_id}<textarea value={operationEvidence} onChange={e=>setOperationEvidence(e.target.value)} className="block w-full rounded border bg-transparent p-2" /></label><button disabled={busy||!operationEvidence.trim()} onClick={()=>void submit({action:'operation_reconcile',operation:op.operation_id,version:op.version,state:'succeeded',evidence:operationEvidence},'External success recorded from your evidence.')}>Record verified success</button><button disabled={busy||!operationEvidence.trim()} onClick={()=>void submit({action:'operation_reconcile',operation:op.operation_id,version:op.version,state:'failed',evidence:operationEvidence},'Final failure recorded; no retry was launched.')}>Record final failure</button></>}{['unknown','failed'].includes(op.state)&&<button disabled={busy} onClick={()=>void submit({action:'operation_probe',operation:op.operation_id,version:op.version},'Trusted probe recorded; inspect the updated outcome.')}>Check external outcome with trusted probe</button>}</section>)}</details>}
          <details><summary>Add a new project direction</summary><label>Instruction<textarea className="block w-full rounded border bg-transparent p-2" rows={3} value={guidance} onChange={e => setGuidance(e.target.value)} /></label><button disabled={busy || !guidance.trim()} onClick={() => void submit({action: 'feedback', text: guidance, task: '*', advisory: false}, 'Project guidance recorded and queued for propagation.')}>Record guidance</button></details>
          <details><summary>Versioned graph patch</summary><label>Patch JSON<textarea className="block w-full rounded border bg-transparent p-2 font-mono text-xs" rows={7} value={patch} onChange={e => { setPatch(e.target.value); setReviewedPatch(''); setReconcileDigest(''); }} /></label><button disabled={busy} onClick={async () => { try { const body: unknown = JSON.parse(patch); const result = await mutateDecisions({action: 'patch_preview', patch: body}); setPreview(JSON.stringify(result, null, 2)); const projection = (result as {projection?: {changed?: boolean; source_digest?: string}}).projection; setReconcileDigest(projection?.changed ? projection.source_digest ?? '' : ''); setReviewedPatch(patch); } catch (e) { setError(String(e)); } }}>Preview impact</button><button disabled={busy || !reviewedPatch || reviewedPatch !== patch} onClick={() => void submit({action: 'patch_apply', patch: JSON.parse(patch) as unknown}, 'Graph patch committed; projection/recovery is durable.')}>Apply reviewed patch</button>{reconcileDigest && <button disabled={busy || reviewedPatch !== patch} onClick={() => void submit({action: 'patch_reconcile', patch: JSON.parse(patch) as unknown, source_digest: reconcileDigest}, 'Inspected external PRD changes retained in the journal; reviewed graph projected.')}>Reconcile inspected PRD with this patch</button>}<pre className="max-h-60 overflow-auto whitespace-pre-wrap text-xs">{preview}</pre></details>
        </div></details>}
      </div>
      <footer className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-t border-[var(--border)] pt-3"><span className="text-xs text-[var(--text-secondary)]">Tab to move · Enter on Send to submit</span><div className="flex gap-2"><Dialog.Close className="rounded border px-3 py-2">{current?.status === 'pending_human' && !neverAsk ? 'Decide later' : 'Close'}</Dialog.Close>{current?.status === 'pending_human' && !neverAsk && <button form="decision-response" type="submit" disabled={busy || !fresh || fresh.version !== current.version || fresh.status !== 'pending_human'} className="rounded-lg bg-[var(--accent)] px-4 py-2 font-medium text-[var(--bg-primary)] disabled:opacity-50">{busy ? 'Recording…' : 'Send answer'}</button>}</div></footer>
    </Dialog.Content></Dialog.Portal>
  </Dialog.Root>;
}
