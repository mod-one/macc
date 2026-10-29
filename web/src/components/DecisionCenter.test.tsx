import {act,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {beforeEach,expect,it,vi} from 'vitest';
import {DecisionCenter} from './DecisionCenter';
import type {Decision,DecisionSnapshot} from '../api/decisions';
const api=vi.hoisted(()=>({get:vi.fn(),mutate:vi.fn()}));
vi.mock('../api/decisions',()=>({getDecisions:api.get,mutateDecisions:api.mutate,getTaskDecisionContexts:vi.fn().mockResolvedValue([])}));
const decision:Decision={id:'question-1',version:1,status:'pending_human',effective_risk:'high',request:{task_id:'A',kind:'decision',question:'Quel contrat public conserver ?',context:'Deux clients consomment le contrat.',recommendation:'Conserver SQLite.',recommended_option_id:'sqlite',options:[{id:'sqlite',label:'Conserver SQLite',consequences:['Contrat conservé']}],affected_tasks:['A','B']},resolution:null};
function snapshot(mode:'high'|'never_ask'='high'):DecisionSnapshot{return{enabled:true,policy:{enabled:true,human_question_min_risk:mode,max_context_bytes:32768,max_request_bytes:65536,trust_mode:'local_trust',mandatory_human_triggers:[]},decisions:[structuredClone(decision)],events:[],plan:{id:'plan',graph_version:1,decision_epoch:1}};}
beforeEach(()=>{sessionStorage.clear();api.get.mockReset();api.mutate.mockReset();api.get.mockResolvedValue(snapshot());api.mutate.mockResolvedValue({});});
it('shows options and saves exact human conditions only after explicit send',async()=>{
    render(<DecisionCenter/>);expect(await screen.findByRole('dialog')).toBeInTheDocument();
    const answer=screen.getByLabelText('Answer / conditions / remarks');fireEvent.change(answer,{target:{value:'Garder SQLite.\nNe pas modifier le contrat.'}});expect(api.mutate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button',{name:'Send answer'}));await waitFor(()=>expect(api.mutate).toHaveBeenCalledWith(expect.objectContaining({action:'answer',version:1,text:'Garder SQLite.\nNe pas modifier le contrat.',option:'sqlite'})));
});
it('deferring preserves the draft and never submits',async()=>{
    render(<DecisionCenter/>);await screen.findByRole('dialog');fireEvent.change(screen.getByLabelText('Answer / conditions / remarks'),{target:{value:'Brouillon à conserver'}});fireEvent.click(screen.getByRole('button',{name:'Decide later'}));expect(screen.queryByRole('dialog')).not.toBeInTheDocument();expect(api.mutate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button',{name:'Decisions, 1 pending'}));expect(screen.getByLabelText('Answer / conditions / remarks')).toHaveValue('Brouillon à conserver');expect(sessionStorage.getItem('macc.decisionDrafts')).toContain('Brouillon à conserver');
});
it('never_ask offers read-only inspection without automatic answer popup',async()=>{
    api.get.mockResolvedValue(snapshot('never_ask'));render(<DecisionCenter/>);await screen.findByRole('button',{name:'Decisions, 1 pending'});expect(screen.queryByRole('dialog')).not.toBeInTheDocument();fireEvent.click(screen.getByRole('button',{name:'Decisions, 1 pending'}));expect(await screen.findByText(/Full autonomy is active/)).toBeInTheDocument();expect(screen.queryByRole('button',{name:'Send answer'})).not.toBeInTheDocument();expect(api.mutate).not.toHaveBeenCalled();
});
it('does not steal focus when another input is being edited',async()=>{
    const input=document.createElement('input');document.body.append(input);input.focus();try{await act(async()=>{render(<DecisionCenter/>);});expect(screen.queryByRole('dialog')).not.toBeInTheDocument();expect(document.activeElement).toBe(input);}finally{input.remove();}
});
it('ignores malformed stored drafts safely',async()=>{sessionStorage.setItem('macc.decisionDrafts','{"question-1@1":null}');render(<DecisionCenter/>);expect(await screen.findByLabelText('Answer / conditions / remarks')).toHaveValue('');});
