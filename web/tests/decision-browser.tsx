// Local browser-only fixture. Never mounts in the shipped application entry.
import React from 'react';
import {createRoot} from 'react-dom/client';
import '../src/index.css';
import {DecisionCenter} from '../src/components/DecisionCenter';
import type {DecisionSnapshot} from '../src/api/decisions';
const snapshot:DecisionSnapshot={enabled:true,policy:{enabled:true,human_question_min_risk:'high',trust_mode:'local_trust',max_context_bytes:262144,max_request_bytes:262144,mandatory_human_triggers:[]},plan:{id:'layout',graph_version:1,decision_epoch:1},events:[],decisions:[{id:'layout-question',version:1,status:'pending_human',effective_risk:'high',request:{task_id:'A',kind:'decision',question:'Which public contract must both consumers preserve?',context:'Long context with Unicode éà and a URL-like unbroken reference '+ 'reference'.repeat(60)+'\n'+('Context with explicit consequences. '.repeat(80)),recommendation:'Preserve all existing data.',options:Array.from({length:12},(_,i)=>({id:`choice-${i}`,label:`Alternative ${i}`,consequences:['Preserve compatibility and existing data.']})),recommended_option_id:'choice-0',affected_tasks:['A','B']},resolution:null}]};
if(new URLSearchParams(location.search).get('scenario')==='intervention') {
 const request=snapshot.decisions[0].request as typeof snapshot.decisions[0]['request'] & {intervention?:unknown;response_fields?:unknown[]};
 request.kind='intervention';request.question='Reconnect the device so the performer can finish verification';request.options=[];
 request.intervention={action:'Reconnect the USB device and restore its existing ADB forward on port 5037.',inaccessible_reason:'The performer has no physical USB access.',expected_result:'Return the forwarded port and the actual adb get-state output.',verification:'The performer will check the listener and device state independently.'};
 request.response_fields=[{id:'port_output',label:'Forwarded port and ADB output',required:true,multiline:true,choices:[]}];
}
const scenario=new URLSearchParams(location.search).get('scenario');
if(scenario==='simple-choice'||scenario==='simple-boolean') {
 const request=snapshot.decisions[0].request;
 request.question='Preserve the public API contract?';
 request.options=scenario==='simple-choice'?[{id:'yes',label:'Yes',consequences:[]},{id:'no',label:'No',consequences:[]}]:[];
 request.recommended_option_id=scenario==='simple-choice'?'yes':null;
 if(scenario==='simple-boolean')request.response_fields=[{id:'ready',label:'Is access restored?',required:true,multiline:false,choices:['yes','no']}];
}
// Each browser case is a fresh request; never clear another page's stored drafts.
snapshot.decisions[0].id=`layout-${crypto.randomUUID()}`;
let mutations=0;
window.fetch=async(_input,init)=>{
 if(init?.method&&init.method!=='GET') {
  mutations++;
  if(scenario!=='simple-choice'&&scenario!=='simple-boolean')throw new Error('This layout fixture must never submit');
  const input=JSON.parse(String(init.body));const d=snapshot.decisions[0];
  if(input.action!=='answer'||input.id!==d.id||input.version!==d.version||d.status!=='pending_human')throw new Error('Invalid/duplicate fixture response');
  if(scenario==='simple-boolean'&&(input.text!=='yes'||input.fields?.ready!=='yes'))throw new Error('The chosen value was not sent exactly');
  d.status='resolved';d.resolution={text:input.text,selected_option_id:input.option,actor:'fixture-operator',origin:'human_local_trust',recorded_at:'2026-11-07T12:00:00Z',advisory:false,fields:input.fields};
  return new Response(JSON.stringify(d),{status:200,headers:{'Content-Type':'application/json'}});
 }
 return new Response(JSON.stringify(snapshot),{status:200,headers:{'Content-Type':'application/json'}});
};
createRoot(document.getElementById('root')!).render(<React.StrictMode><DecisionCenter/></React.StrictMode>);
const wait=()=>new Promise(resolve=>setTimeout(resolve,25));
(async()=>{try{
 let dialog:HTMLElement|null=null;for(let i=0;i<200&&!dialog;i++){await wait();dialog=document.querySelector('[role="dialog"]');}if(!dialog)throw new Error('Dialog never became available');
 const box=dialog.getBoundingClientRect();if(box.left<0||box.right>innerWidth+1||box.bottom>innerHeight+1)throw new Error('Dialog exceeds viewport');
 if(dialog.scrollWidth>dialog.clientWidth+1)throw new Error('Normal form has horizontal overflow');
 if(scenario==='simple-choice'||scenario==='simple-boolean') {
   if(scenario==='simple-choice')document.querySelector<HTMLInputElement>('input[type="radio"]')!.click();
   else [...document.querySelectorAll('button')].find(b=>b.textContent==='yes')!.click();
   await wait();[...document.querySelectorAll('button')].find(b=>b.textContent==='Send answer')!.click();
   for(let i=0;i<100&&document.querySelector('[role="dialog"]');i++)await wait();
   if(document.querySelector('[role="dialog"]'))throw new Error('Recorded answer did not close the popup');
   if(mutations!==1)throw new Error('Expected exactly one answer');
   if(!document.querySelector('[role="status"]')?.textContent?.includes('Response sent.'))throw new Error('Simple receipt missing');
   document.getElementById('qualification-result')!.textContent=JSON.stringify({status:'passed',width:innerWidth,height:innerHeight,closed_after_recording:true,mutations});return;
 }
 const answer=document.querySelector('textarea')!;const text='Brouillon éà.\nNe pas supprimer de fichiers.';Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value')!.set!.call(answer,text);answer.dispatchEvent(new Event('input',{bubbles:true}));await wait();
 const later=[...document.querySelectorAll('button')].find(b=>b.textContent==='Decide later');if(!later)throw new Error('Explicit defer action missing');later.click();await wait();document.querySelector<HTMLButtonElement>('button[aria-label^="Decisions,"]')!.click();await wait();
 if(document.querySelector('textarea')?.value!==text)throw new Error('Deferred draft was lost');if(mutations)throw new Error('Closing or typing submitted a mutation');
 const send=[...document.querySelectorAll('button')].find(b=>b.textContent==='Send answer');
 if(!send)throw new Error('Primary send action missing');const sendBox=send.getBoundingClientRect();if(sendBox.top<0||sendBox.bottom>innerHeight+1)throw new Error('Send action is not visible');
 let required_form_validation=false;
 if(new URLSearchParams(location.search).get('scenario')==='intervention') {
   send.click();await wait();await wait();
   const confirmation=document.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
   if(confirmation.checked)throw new Error('Typing a result inferred that an action was performed');
   if(document.activeElement!==confirmation)throw new Error('First missing visible field was not focused');
   confirmation.click();send.click();await wait();await wait();
   const field=document.getElementById('decision-response-port_output')!;
   if(document.activeElement!==field)throw new Error('Missing required field was not focused');
   const fieldBox=field.getBoundingClientRect();const body=field.closest('form')!.parentElement!.parentElement!.getBoundingClientRect();
   if(fieldBox.top<body.top-1||fieldBox.bottom>body.bottom+1)throw new Error('Focused required field is obscured by the mobile footer');
   if(document.querySelector('textarea')?.value!==text)throw new Error('Invalid submission lost the observed result');
   if(mutations)throw new Error('Invalid response reached the server');required_form_validation=true;
 }
 document.getElementById('qualification-result')!.textContent=JSON.stringify({status:'passed',width:innerWidth,height:innerHeight,draft_preserved:true,required_form_validation,send_visible:true,mutations});
}catch(error){document.getElementById('qualification-result')!.textContent=JSON.stringify({status:'failed',error:String(error)});}})();
