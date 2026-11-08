import {expect,it} from 'vitest';
import type {Decision} from '../api/decisions';
import {decisionAnswer,responseProblems,type DecisionDraft} from './DecisionResponseForm';
import {changeResponseMode} from './decisionResponseMode';
const request=():Decision=>({id:'a',version:1,status:'pending_human',effective_risk:'high',resolution:null,request:{task_id:'A',kind:'intervention',question:'Provide device or defer?',context:'No USB',recommendation:'Provide',recommended_option_id:'provide',affected_tasks:['A'],options:[{id:'provide',label:'Provide access',consequences:[],intervention_response:'completed'},{id:'defer',label:'Keep unverified',consequences:[],intervention_response:'defer_observation'}],response_fields:[{id:'device',label:'Actual device output',required:true,multiline:true,choices:[]}]}});
const draft=():DecisionDraft=>({text:'',option:'defer',advisory:false});
it('sends an explicit typed deferral without inventing an action, text or observations',()=>{
  const r=request(),d=draft();expect(responseProblems(r,d)).toEqual({});
  const sent=decisionAnswer(r,d);expect(sent.option).toBe('defer');expect(sent.text).toContain('unverified');expect(sent.fields).not.toHaveProperty('performed');
});
it('still requires real results and requested device output for a completed action',()=>{
  const r=request(),d={...draft(),option:'provide'};
  expect(responseProblems(r,d)).toHaveProperty('performed');expect(responseProblems(r,d)).toHaveProperty('result');expect(responseProblems(r,d)).toHaveProperty('device');
});
it('explicitly changing a legacy provide response clears contradictory claims',()=>{
  const r=request();delete r.request.options[0].intervention_response;
  const d={...draft(),option:'provide',fields:{performed:'yes',result:'yes'}};
  const next={...d,...changeResponseMode(r,d,'defer_observation')};
  expect(next.option).toBe('');expect(next.fields).toEqual({intervention_response:'defer_observation'});expect(responseProblems(r,next)).toEqual({});
});
it('fact confirmation preserves required facts and inability uses rejection',()=>{
  const r=request(),d={...draft(),option:'',fields:{intervention_response:'confirm_fact'}};
  expect(responseProblems(r,d)).toHaveProperty('device');expect(responseProblems(r,d)).not.toHaveProperty('performed');
  const unable={...d,...changeResponseMode(r,d,'unable')};expect(unable.outcome).toBe('reject');expect(responseProblems(r,unable)).toEqual({});
});
