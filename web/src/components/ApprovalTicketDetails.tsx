import type {ApprovalTicket} from '../api/decisions';
const object=(value:unknown):Record<string,unknown>=>value&&typeof value==='object'&&!Array.isArray(value)?value as Record<string,unknown>:{};
const text=(value:unknown)=>typeof value==='string'?value:'';
/** Display the immutable action, even if the operator subsequently edits a draft. */
export function ApprovalTicketDetails({ticket}:{ticket:ApprovalTicket}){
 const intent=ticket.intent,input=object(intent.input),record=object(intent.record),request=object(intent.request),policy=object(intent.policy);const fields=object(input.fields);
 return <section className="rounded border p-2 text-sm"><h4>Exact action submitted for independent approval</h4><p>{text(intent.action)} · {text(intent.id)||text(intent.task)||text(intent.operation)||text(intent.plan_id)}</p>
  {text(request.question)&&<p className="whitespace-pre-wrap">{text(request.question)}</p>}{text(input.text)&&<p className="whitespace-pre-wrap">{text(input.text)}</p>}{text(input.option)&&<p>Selected option: {text(input.option)}</p>}{text(input.effects)&&<p>Task-plan effect: {text(input.effects)}</p>}
  {Object.entries(fields).map(([key,value])=><p key={key} className="whitespace-pre-wrap">{key}: {text(value)}</p>)}
  {text(record.actor)&&<p>{text(record.decision)} by {text(record.actor)} ({text(record.role)}) · Revision {text(record.revision)}</p>}{text(record.evidence)&&<p className="whitespace-pre-wrap">Evidence: {text(record.evidence)}</p>}{text(record.reason)&&<p className="whitespace-pre-wrap">Reason: {text(record.reason)}</p>}
  {text(intent.reason)&&<p className="whitespace-pre-wrap">Reason: {text(intent.reason)}</p>}{text(intent.evidence)&&<p className="whitespace-pre-wrap">Evidence: {text(intent.evidence)}</p>}{text(policy.human_question_min_risk)&&<p>Autonomy: {text(policy.human_question_min_risk)}</p>}
  <p className="text-xs">This approval applies to this recorded action. Subsequent draft edits need a new review.</p><details><summary>Complete submitted action</summary><pre className="max-h-40 overflow-auto whitespace-pre-wrap text-xs">{JSON.stringify(intent,null,2)}</pre></details>
 </section>;
}
