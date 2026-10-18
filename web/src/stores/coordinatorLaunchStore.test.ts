import { afterEach, expect, it, vi } from 'vitest';
import { getCoordinatorLaunchReview, getStatus, launchCoordinator, postCoordinatorAction } from '../api/client';
import { useCoordinatorStore } from '../store';
import { closeCoordinatorLaunch, useCoordinatorLaunchStore } from './coordinatorLaunchStore';
vi.mock('../api/client',()=>({getCoordinatorLaunchReview:vi.fn(),getStatus:vi.fn(),launchCoordinator:vi.fn(),postCoordinatorAction:vi.fn()}));
afterEach(()=>{closeCoordinatorLaunch();vi.clearAllMocks();});
it('all run entry points wait for a reviewed decision before POST',async()=>{
  vi.mocked(getCoordinatorLaunchReview).mockResolvedValue({revision:'r1',project:'p',taskSource:'tasks.json',queue:[],referenceBranch:'main',tools:[],maxParallel:1,maxDispatch:1,needsBranchCreation:false,branchSources:[],dirtyReference:false,warnings:[],blockers:[]});
  vi.mocked(launchCoordinator).mockResolvedValue({resumed:true});
  vi.mocked(getStatus).mockResolvedValue({} as Awaited<ReturnType<typeof getStatus>>);
  const result=useCoordinatorStore.getState().runAction('run');
  expect(useCoordinatorLaunchStore.getState().open).toBe(true);
  expect(launchCoordinator).not.toHaveBeenCalled();expect(postCoordinatorAction).not.toHaveBeenCalled();
  closeCoordinatorLaunch({confirmed:true,revision:'r1',supervisor:true,createFrom:null,allowDirtyReference:false});
  await expect(result).resolves.toEqual({resumed:true});
  expect(launchCoordinator).toHaveBeenCalledWith(expect.objectContaining({supervisor:true,confirmed:true}));
});
it('cancelled run never reaches the launch endpoint',async()=>{
  vi.mocked(getCoordinatorLaunchReview).mockResolvedValue({} as Awaited<ReturnType<typeof getCoordinatorLaunchReview>>);
  const result=useCoordinatorStore.getState().runAction('run');
  const rejected=expect(result).rejects.toMatchObject({name:'AbortError'});
  closeCoordinatorLaunch();await rejected;
  expect(launchCoordinator).not.toHaveBeenCalled();expect(useCoordinatorStore.getState().pendingAction).toBeNull();
});
