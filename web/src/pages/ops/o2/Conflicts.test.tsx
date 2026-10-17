import { fireEvent, render, screen, waitFor, act } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import Conflicts from './Conflicts';
import { o2Action } from '../../../api/o2';
vi.mock('../../../api/o2', () => ({ o2Action: vi.fn() }));
const conflict={id:'conflict-1',left:'left-id',right:'right-id',reason:'Contradiction',status:'open',checksum:'pair-sha',left_access:{task_id:'A'},right_access:{task_id:'B'}};
beforeEach(()=>{vi.resetAllMocks();vi.mocked(o2Action).mockImplementation(async action=>action==='memory_conflicts'?[conflict]:{memory:{id:'claim',proposal:{body:'claim evidence'}}});});
async function inspect(){fireEvent.click(screen.getByRole('button',{name:'Inspect memory conflicts'}));fireEvent.click(await screen.findByRole('button',{name:'Inspect conflict conflict-1'}));}
it('requires reading both claims and sends the exact reviewed conflict checksum and explicit outcome',async()=>{
  render(<Conflicts identity="project" access={{}}/>);await inspect();
  expect(screen.getByRole('checkbox')).toBeDisabled();
  fireEvent.click(screen.getByRole('button',{name:'Read both claims'}));await waitFor(()=>expect(screen.getByRole('checkbox')).toBeEnabled());
  expect(o2Action).toHaveBeenCalledWith('memory_get',{id:'left-id',access:{task_id:'A'}});expect(o2Action).toHaveBeenCalledWith('memory_get',{id:'right-id',access:{task_id:'B'}});
  fireEvent.change(screen.getByLabelText('Conflict outcome'),{target:{value:'keep_left'}});
  fireEvent.change(screen.getByLabelText('Conflict resolution reason'),{target:{value:'Validated source evidence'}});
  fireEvent.click(screen.getByRole('checkbox'));fireEvent.click(screen.getByRole('button',{name:'Resolve reviewed conflict'}));
  await waitFor(()=>expect(o2Action).toHaveBeenCalledWith('memory_resolve_conflict',{id:'conflict-1',reason:'Validated source evidence',confirmed:true,resolution:'keep_left',expected_checksum:'pair-sha'}));
});
it('discards pending claim reads after a scope change without restoring confirmation',async()=>{
  const view=render(<Conflicts identity="A" access={{task_id:'A'}}/>);await inspect();
  let resolve!:(value:unknown)=>void;vi.mocked(o2Action).mockImplementation(()=>new Promise(r=>{resolve=r;}));
  fireEvent.click(screen.getByRole('button',{name:'Read both claims'}));view.rerender(<Conflicts identity="B" access={{task_id:'B'}}/>);
  await act(async()=>{resolve({memory:{proposal:{body:'obsolete claim'}}});});
  expect(screen.queryByText(/obsolete claim/)).not.toBeInTheDocument();expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
  expect(vi.mocked(o2Action).mock.calls.filter(([action])=>action==='memory_get')).toHaveLength(1);
});
