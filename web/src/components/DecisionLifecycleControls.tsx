import React from 'react';
import type {Decision} from '../api/decisions';
interface Draft {question:string;context:string;recommendation:string;reason:string;expires:string;patch:string;key:string}
function initial(d:Decision):Draft {
  const key=`macc.lifecycle.${d.id}@${d.version}`;
  try {const value=JSON.parse(sessionStorage.getItem(key)??'null');if(value&&['question','context','recommendation','reason','expires','patch','key'].every(k=>typeof value[k]==='string'))return value;}catch{/* Restore only validated drafts. */}
  return {question:d.request.question,context:d.request.context,recommendation:d.request.recommendation,reason:'',expires:'',patch:'',key:crypto.randomUUID()};
}
export function DecisionLifecycleControls({decision:d,busy,run,onError}:{decision:Decision;busy:boolean;run:(intent:Record<string,unknown>,message:string)=>Promise<void>;onError:(message:string)=>void}){
  const [mode,setMode]=React.useState('');const [draft,setDraft]=React.useState(()=>initial(d));
  React.useEffect(()=>{try{sessionStorage.setItem(`macc.lifecycle.${d.id}@${d.version}`,JSON.stringify(draft));}catch{/* Keep the in-memory draft. */}},[draft,d.id,d.version]);
  if(['revoked','withdrawn','superseded'].includes(d.status))return null;
  const revise=async()=>{
    try {
      const request={...d.request,idempotency_key:`revision-${draft.key}`,revision_of:null,supersedes:[],subject_revision:null,proposed_patch:null,question:draft.question,context:draft.context,recommendation:draft.recommendation,expires_at:draft.expires?new Date(draft.expires).toISOString():null,options:d.request.options.map(o=>({...o,requires_replan:!!o.proposed_patch||o.requires_replan,proposed_patch:null}))};
      await run({action:'revise',id:d.id,version:d.version,request},'Revised proposal recorded. Previous answers remain in history; no consent was inferred.');
    }catch(e){onError(String(e));}
  };
  return <section className="space-y-2 rounded border p-3"><h4>Revise or manage this request</h4>
    <div className="flex flex-wrap gap-2"><button disabled={busy} onClick={()=>setMode(mode==='revise'?'':'revise')}>{d.status==='resolved'?'Propose replacement':'Revise request'}</button>
      <button disabled={busy} onClick={()=>setMode(mode==='reason'?'':'reason')}>{d.status==='resolved'?'Revoke decision':'Withdraw request'}</button>
      {d.status==='awaiting_effects'&&<button disabled={busy} onClick={()=>setMode(mode==='effects'?'':'effects')}>Review a prepared task plan</button>}</div>
    {mode==='revise'&&<div className="space-y-2"><p>The revised proposal gets a new versioned identity. A previous rejection requires a new explicit answer.</p>
      <label>Revised question<textarea value={draft.question} onChange={e=>setDraft(v=>({...v,question:e.target.value}))} className="block w-full rounded border bg-transparent p-2"/></label>
      <label>Revised context<textarea value={draft.context} onChange={e=>setDraft(v=>({...v,context:e.target.value}))} className="block w-full rounded border bg-transparent p-2"/></label>
      <label>Revised recommendation<textarea value={draft.recommendation} onChange={e=>setDraft(v=>({...v,recommendation:e.target.value}))} className="block w-full rounded border bg-transparent p-2"/></label>
      <label>Proposal expiry (optional)<input type="datetime-local" value={draft.expires} onChange={e=>setDraft(v=>({...v,expires:e.target.value}))}/></label>
      <button disabled={busy||!draft.question.trim()||!draft.recommendation.trim()} onClick={()=>void revise()}>Submit revised proposal</button></div>}
    {mode==='reason'&&<div><label>{d.status==='resolved'?'Revocation reason':'Withdrawal reason'}<textarea value={draft.reason} onChange={e=>setDraft(v=>({...v,reason:e.target.value}))} className="block w-full rounded border bg-transparent p-2"/></label>
      <p>This preserves history and does not approve an operation or another proposal.</p><button disabled={busy||!draft.reason.trim()} onClick={()=>void run({action:d.status==='resolved'?'revoke':'withdraw',id:d.id,version:d.version,reason:draft.reason},'Request lifecycle change durably recorded.')}>{d.status==='resolved'?'Confirm revocation':'Confirm withdrawal'}</button></div>}
    {mode==='effects'&&<div><p>The coordinator can prepare a bounded plan automatically. An operator may also review and apply a prepared patch here.</p><label>Prepared task-plan patch<textarea value={draft.patch} onChange={e=>setDraft(v=>({...v,patch:e.target.value}))} className="block w-full rounded border bg-transparent p-2 font-mono"/></label><button disabled={busy||!draft.patch.trim()} onClick={()=>{try{void run({action:'effects',id:d.id,version:d.version,patch:JSON.parse(draft.patch)},'Recorded answer and task-plan effects are now applied; affected tasks may resume with fresh contexts.');}catch(e){onError(String(e));}}}>Apply reviewed task plan</button></div>}
  </section>;
}
