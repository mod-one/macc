import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import ApplyPreparation from './ApplyPreparation';
import type { ApiPreparationReport } from '../api/models';
import { allowApplyNetwork } from '../api/client';
vi.mock('../api/client', () => ({allowApplyNetwork: vi.fn()}));
const base: ApiPreparationReport = {revision:'current',status:'waiting_for_input',completed:['Schema verified'],error:null,question:null};
it('keeps a failed benchmark blocked and shows its evidence', () => {
  const resume=vi.fn();
  render(<ApplyPreparation report={{...base,question:{kind:'semantic_review',message:'Review proof',data:{approvable:false,report:{id:'failed',passed:false},checksum:'proof'}}}} busy={false} resume={resume} />);
  expect(screen.getByRole('button',{name:/Approve these benchmark/})).toBeDisabled();
  expect(screen.getByText(/"passed": false/)).toBeInTheDocument();
  expect(resume).not.toHaveBeenCalled();
});
it('binds passing approval to the exact report and checksum', async () => {
  const resume=vi.fn().mockResolvedValue(undefined);
  render(<ApplyPreparation report={{...base,question:{kind:'semantic_review',message:'Review proof',data:{approvable:true,report:{id:'passed',passed:true},checksum:'exact'}}}} busy={false} resume={resume} />);
  fireEvent.click(screen.getByRole('button',{name:/Approve these benchmark/}));
  await waitFor(()=>expect(resume).toHaveBeenCalledWith({semantic_approval:{report_id:'passed',checksum:'exact'}}));
});
it('validates supplied JSON before refreshing the reviewed Apply plan', async () => {
  const resume=vi.fn().mockResolvedValue(undefined);
  render(<ApplyPreparation report={{...base,question:{kind:'semantic_suite',message:'Supply labels',data:{}}}} busy={false} resume={resume} />);
  const input=screen.getByRole('textbox');
  fireEvent.change(input,{target:{value:'bad json'}});
  fireEvent.click(screen.getByRole('button',{name:/Validate suite/}));
  expect(await screen.findByRole('alert')).toBeInTheDocument();
  expect(resume).not.toHaveBeenCalled();
  fireEvent.change(input,{target:{value:'{"cases":[]}'}});
  fireEvent.click(screen.getByRole('button',{name:/Validate suite/}));
  await waitFor(()=>expect(resume).toHaveBeenCalledWith({semantic_suite:{cases:[]}}));
});
it('asks before allowing network and uses the report revision', async () => {
  const resume=vi.fn().mockResolvedValue(undefined);
  vi.mocked(allowApplyNetwork).mockResolvedValue(undefined);
  render(<ApplyPreparation report={{...base,question:{kind:'network_access',message:'Allow network?',data:{}}}} busy={false} resume={resume} />);
  expect(allowApplyNetwork).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button',{name:/Allow project network/}));
  await waitFor(()=>expect(allowApplyNetwork).toHaveBeenCalledWith('current'));
  expect(resume).toHaveBeenCalledWith({});
});
