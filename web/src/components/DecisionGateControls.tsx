import React from 'react';
import {ApprovalConditions,validApprovalConditions} from './DecisionEvidence';
import type {ApprovalCondition} from '../api/decisions';
import type {ApiPendingApproval} from '../api/models';
export function DecisionGateControls({gates,plan,identity,busy,run}:{gates:ApiPendingApproval[];plan:string;identity:string;busy:boolean;run:(intent:Record<string,unknown>,message:string)=>Promise<void>}){
 return <>{gates.filter(g=>g.awaiting_decision).map(g=><Gate key={`${g.task_id}@${g.subject_revision}`} gate={g} plan={plan} identity={identity} busy={busy} run={run}/>)}</>;
}
interface GateDraft {conditions:ApprovalCondition[];actor:string;role:string;decision:string;evidence:string;reason:string}
function Gate({gate:g,plan,identity,busy,run}:{gate:ApiPendingApproval;plan:string;identity:string;busy:boolean;run:(intent:Record<string,unknown>,message:string)=>Promise<void>}){
 const key=`macc.gate.${g.task_id}@${g.subject_revision}`;
 const [draft,setDraft]=React.useState<GateDraft>(()=>{const initial={conditions:[] as ApprovalCondition[],actor:identity,role:g.missing_roles[0]??g.required_roles[0]??'',decision:'approved',evidence:'',reason:''};try{const saved=JSON.parse(sessionStorage.getItem(key)??'null');return saved&&['actor','role','decision','evidence','reason'].every(k=>typeof saved[k]==='string')&&validApprovalConditions(saved.conditions??[])?{...initial,...saved}:initial;}catch{return initial;}});
 React.useEffect(()=>{try{sessionStorage.setItem(key,JSON.stringify(draft));}catch{/* Keep draft in memory. */}},[key,draft]);
 return <details className="space-y-2 rounded border p-3"><summary>Approval gate: {g.question}</summary><p>{g.description}</p><p>Subject: {g.subject_task} · Revision: {g.subject_revision??'Unavailable'} · Roles: {g.required_roles.join(', ')}</p>
  <label>Approver identity<input value={draft.actor} onChange={e=>setDraft(v=>({...v,actor:e.target.value}))} className="block w-full rounded border bg-transparent p-2"/></label><p className="text-xs">In secure mode this must match the independently authenticated principal. A typed identity alone grants no authority.</p>
  <label>Approval role<select value={draft.role} onChange={e=>setDraft(v=>({...v,role:e.target.value}))}>{g.required_roles.map(role=><option key={role}>{role}</option>)}</select></label>
  <label>Gate response<select value={draft.decision} onChange={e=>setDraft(v=>({...v,decision:e.target.value}))}><option value="approved">Approve</option><option value="rejected">Reject</option><option value="changes_requested">Request changes</option></select></label>
  <label>Gate evidence<textarea value={draft.evidence} onChange={e=>setDraft(v=>({...v,evidence:e.target.value}))} className="block w-full rounded border bg-transparent p-2"/></label><label>Gate reason<textarea value={draft.reason} onChange={e=>setDraft(v=>({...v,reason:e.target.value}))} className="block w-full rounded border bg-transparent p-2"/></label>
  {draft.decision==='approved'&&<ApprovalConditions value={draft.conditions??[]} onChange={conditions=>setDraft(v=>({...v,conditions}))}/>}
  <button disabled={busy||!g.subject_revision||!draft.actor.trim()||!draft.role||(draft.decision==='approved'?g.evidence_required&&!draft.evidence.trim():!draft.reason.trim())} onClick={()=>void run({action:draft.decision==='approved'&&draft.conditions?.length?'conditional_gate_vote':'gate_vote',...(draft.decision==='approved'&&draft.conditions?.length?{conditions:draft.conditions}:{}),plan_id:plan,task:g.task_id,record:{decision:draft.decision,role:draft.role,actor:draft.actor.trim(),revision:g.subject_revision,evidence:draft.evidence.trim()||null,reason:draft.reason.trim()||null,recorded_at:new Date().toISOString()}},'Gate response durably recorded; current quorum is re-evaluated by the coordinator.')}>Submit gate response</button>
 </details>;
}
