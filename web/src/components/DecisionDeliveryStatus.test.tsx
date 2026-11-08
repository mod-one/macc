import {render,screen} from '@testing-library/react';
import {it,expect} from 'vitest';
import {DecisionDeliveryStatus} from './DecisionDeliveryStatus';
import type {Decision,DecisionSnapshot} from '../api/decisions';
it('never presents a previous answer version as verified delivery of the current response',()=>{
  const current={id:'d',version:2,resolution:{text:'New answer'}} as Decision;
  const snapshot={enabled:true,policy:{enabled:true,human_question_min_risk:"high",trust_mode:"local_trust",max_context_bytes:131072,max_request_bytes:65536,mandatory_human_triggers:[]},decisions:[],events:[],plan:null,delivery:[{task:'A',phase:'review',current:false,delivered:true,acknowledged:true,decisions:[{id:'d',version:1}],compliance:{d:{status:'pass',evidence:'Old answer'}}}]} as DecisionSnapshot;
  const {rerender}=render(<DecisionDeliveryStatus current={current} snapshot={snapshot}/>);
  expect(screen.getByText(/Delivery pending/)).toBeInTheDocument();expect(screen.queryByText('Old answer')).not.toBeInTheDocument();
  snapshot.delivery![0].decisions[0].version=2;
  rerender(<DecisionDeliveryStatus current={current} snapshot={snapshot}/>);
  expect(screen.getByText('Refresh required')).toBeInTheDocument();expect(screen.getByText(/pass Old answer/)).toBeInTheDocument();
});
