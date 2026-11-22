import type {ApprovalCondition, ArtifactRef, Decision, InterventionResponse} from '../api/decisions';

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

export const responseTypes: Record<InterventionResponse,string> = {
  completed:'Action completed — report the result',
  confirm_fact:'Confirm a fact (operator statement)',
  defer_observation:'Explicitly defer observation; keep it unverified',
  unable:'Unable to help — keep dependent work stopped',
};
export function responseMode(current:Decision,draft:DecisionDraft):InterventionResponse {
  return current.request.options.find(o=>o.id===draft.option)?.intervention_response
    ?? draft.fields?.intervention_response as InterventionResponse ?? 'completed';
}
export function responseFieldRequired(current:Decision,draft:DecisionDraft,id:string,required:boolean):boolean {
  if(!required || (draft.outcome??'answer')!=='answer') return false;
  if(current.request.kind!=='intervention') return true;
  const mode=responseMode(current,draft);
  return mode!=='defer_observation' && mode!=='unable' && !(mode==='confirm_fact' && ['performed','result'].includes(id));
}
export function changeResponseMode(current:Decision,draft:DecisionDraft,mode:InterventionResponse):Partial<DecisionDraft> {
  const fields:Record<string,string>={...draft.fields,intervention_response:mode};
  delete fields.performed; delete fields.result;
  const selected=current.request.options.find(o=>o.id===draft.option);
  return {fields, option:selected?.intervention_response===mode?draft.option:'',
    outcome:mode==='unable'?'reject':'answer',applyPatch:false};
}

export function choiceText(current:Decision,draft:DecisionDraft):string {
  if(['intervention','approval','authorization'].includes(current.request.kind))return '';
  const values=(current.request.response_fields??[]).flatMap(field=>{const value=draft.fields?.[field.id];return value&&field.choices.includes(value)?[[field.label,value]]:[];});
  return values.length===1?values[0][1]:values.map(([label,value])=>`${label}: ${value}`).join('\n');
}
export function responseProblems(current: Decision, draft: DecisionDraft): Record<string, string> {
  const errors: Record<string, string> = {};
  const answering = (draft.outcome ?? 'answer') === 'answer';
  if (answering && current.request.kind === 'intervention') {
    if (responseMode(current,draft)==='completed' && draft.fields?.performed !== 'yes') errors.performed = 'Confirm that you performed the action, or choose “I cannot do this”.';
    if (responseMode(current,draft)==='completed' && !draft.fields?.result?.trim()) errors.result = 'Describe the actual result or paste the command output. A general note does not fill the requested result.';
    const mode=responseMode(current,draft);
    const declared=current.request.options.find(o=>o.id===draft.option)?.intervention_response;
    if(declared && draft.fields?.intervention_response && declared!==draft.fields.intervention_response) errors.intervention_response='Refresh the response type: it contradicts the selected option.';
    if(mode!=='completed' && draft.fields?.performed==='yes') errors.performed='A confirmation or deferral cannot claim a physical action.';
    if(mode==='unable') errors.intervention_response='Choose the rejection outcome for inability to help.';
    if (draft.advisory) errors.advisory = 'An intervention result must be explicit; turn off advisory guidance.';
  }
  for (const field of current.request.response_fields ?? []) {
    const value = draft.fields?.[field.id] ?? '';
    if (responseFieldRequired(current,draft,field.id,field.required) && !value.trim()) errors[field.id] = `Please complete: ${field.label}.`;
    else if (value && field.choices.length && !field.choices.includes(value)) errors[field.id] = `Choose one of the listed values for ${field.label}.`;
  }
  if (answering && ['approval', 'authorization'].includes(current.request.kind) && !draft.fields?.evidence?.trim()) errors.evidence = 'Provide the evidence for this approval.';
  if (!draft.text.trim() && !draft.option && !choiceText(current,draft) && !(current.request.kind === 'intervention' && (draft.fields?.result?.trim() || responseMode(current,draft)!=='completed'))) errors.text = 'Write an answer or choose an option.';
  return errors;
}

export function decisionAnswer(current: Decision, draft: DecisionDraft) {
  return {
    action: 'answer', id: current.id, version: current.version,
    text: draft.text.trim() ? draft.text : current.request.kind === 'intervention' ? draft.fields?.result?.trim() || responseTypes[responseMode(current,draft)] : current.request.options.find(o => o.id === draft.option)?.label ?? choiceText(current,draft),
    option: draft.option || null, advisory: draft.advisory, apply_patch: !!draft.applyPatch,
    effects: draft.effects ?? 'auto', conditions: draft.conditions ?? [], artifacts: draft.artifacts ?? [],
    fields: draft.fields ?? {}, outcome: draft.outcome ?? 'answer',
  };
}
