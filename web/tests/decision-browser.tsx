// Local browser-only fixture. Never mounts in the shipped application entry.
import React from 'react';
import {createRoot} from 'react-dom/client';
import '../src/index.css';
import {DecisionCenter} from '../src/components/DecisionCenter';
const snapshot={enabled:true,policy:{enabled:true,human_question_min_risk:'high',trust_mode:'local_trust',max_context_bytes:262144,max_request_bytes:262144,mandatory_human_triggers:[]},plan:{id:'layout',graph_version:1,decision_epoch:1},events:[],decisions:[{id:'layout-question',version:1,status:'pending_human',effective_risk:'high',request:{task_id:'A',kind:'decision',question:'Which public contract must both consumers preserve?',context:'Long context with Unicode éà and a URL-like unbroken reference '+ 'reference'.repeat(60)+'\n'+('Context with explicit consequences. '.repeat(80)),recommendation:'Preserve all existing data.',options:Array.from({length:12},(_,i)=>({id:`choice-${i}`,label:`Alternative ${i}`,consequences:['Preserve compatibility and existing data.']})),recommended_option_id:'choice-0',affected_tasks:['A','B']},resolution:null}]};
let mutations=0;
window.fetch=async(_input,init)=>{if(init?.method&&init.method!=='GET'){mutations++;throw new Error('This layout fixture must never submit');}return new Response(JSON.stringify(snapshot),{status:200,headers:{'Content-Type':'application/json'}});};
createRoot(document.getElementById('root')!).render(<React.StrictMode><DecisionCenter/></React.StrictMode>);
const wait=()=>new Promise(resolve=>setTimeout(resolve,25));
(async()=>{try{
 let dialog:HTMLElement|null=null;for(let i=0;i<200&&!dialog;i++){await wait();dialog=document.querySelector('[role="dialog"]');}if(!dialog)throw new Error('Dialog never became available');
 const box=dialog.getBoundingClientRect();if(box.left<0||box.right>innerWidth+1||box.bottom>innerHeight+1)throw new Error('Dialog exceeds viewport');
 if(dialog.scrollWidth>dialog.clientWidth+1)throw new Error('Normal form has horizontal overflow');
 const answer=document.querySelector('textarea')!;const text='Brouillon éà.\nNe pas supprimer de fichiers.';Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value')!.set!.call(answer,text);answer.dispatchEvent(new Event('input',{bubbles:true}));await wait();
 const later=[...document.querySelectorAll('button')].find(b=>b.textContent==='Decide later');if(!later)throw new Error('Explicit defer action missing');later.click();await wait();document.querySelector<HTMLButtonElement>('button[aria-label^="Decisions,"]')!.click();await wait();
 if(document.querySelector('textarea')?.value!==text)throw new Error('Deferred draft was lost');if(mutations)throw new Error('Closing or typing submitted a mutation');
 document.getElementById('qualification-result')!.textContent=JSON.stringify({status:'passed',width:innerWidth,height:innerHeight,draft_preserved:true,mutations});
}catch(error){document.getElementById('qualification-result')!.textContent=JSON.stringify({status:'failed',error:String(error)});}})();
