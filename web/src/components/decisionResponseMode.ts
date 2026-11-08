import type {Decision, InterventionResponse} from '../api/decisions';
import type {DecisionDraft} from './DecisionResponseForm';
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
