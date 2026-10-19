import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, expect, it, vi } from 'vitest';
import Apply from './Apply';
import { runApply, runPlan } from '../api/client';
import type { ApiPlanResponse } from '../api/models';
vi.mock('../api/client',async()=>{
  const actual=await vi.importActual<typeof import('../api/client')>('../api/client');
  return {...actual,runApply:vi.fn(),runPlan:vi.fn()};
});
const plan:ApiPlanResponse={planRevision:'plan-1',summary:{totalActions:1,filesWrite:1,filesMerge:0,consentRequired:0,backupRequired:1,backupPath:'.macc/backups'},files:[],diffs:[],risks:[],consents:[]};
beforeEach(()=>{vi.mocked(runPlan).mockReset().mockResolvedValue(plan);vi.mocked(runApply).mockReset().mockResolvedValue({dryRun:false,appliedActions:1,changedFiles:1,results:[],backupLocations:[],warnings:[]});});
it('shows SQLite activation and migration backups in the apply confirmation',async()=>{
  vi.mocked(runPlan).mockResolvedValue({...plan,files:[{path:'O2: migrate sessions to SQLite',kind:'other',scope:'project',consentRequired:false,backupRequired:true,setExecutable:false,riskLevel:'caution',contentPreview:null,explain:null}]});
  render(<MemoryRouter><Apply /></MemoryRouter>);
  await screen.findByText('Plan Summary');
  fireEvent.click(screen.getByRole('button',{name:'Apply Changes'}));
  expect(screen.getByRole('alertdialog')).toHaveTextContent('migrate sessions to SQLite');
  expect(screen.getByRole('alertdialog')).toHaveTextContent('Migration backups are retained under .macc/state');
  expect(runApply).not.toHaveBeenCalled();
});
it('requires confirmation even for project-only apply and Cancel sends no write',async()=>{
  render(<MemoryRouter><Apply /></MemoryRouter>);
  await screen.findByText('Plan Summary');
  fireEvent.click(screen.getByRole('button',{name:'Apply Changes'}));
  expect(screen.getByRole('alertdialog')).toBeInTheDocument();expect(runApply).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button',{name:'Cancel'}));expect(runApply).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button',{name:'Apply Changes'}));
  fireEvent.click(screen.getByRole('button',{name:'Confirm Apply'}));
  await waitFor(()=>expect(runApply).toHaveBeenCalledWith({scope:'project',allowUserScope:false,dryRun:false,confirmed:true,planRevision:'plan-1'}));
});
it('renders dry-run previews as plans rather than attempting to read apply results',async()=>{
  vi.mocked(runApply).mockResolvedValue({...plan,summary:{...plan.summary,totalActions:2}});
  render(<MemoryRouter><Apply /></MemoryRouter>);await screen.findByText('Plan Summary');
  fireEvent.click(screen.getByRole('button',{name:'Dry Run'}));
  fireEvent.click(screen.getByRole('button',{name:'Run Dry Run'}));
  await waitFor(()=>expect(screen.getByText('Total Actions').closest('article')).toHaveTextContent('2'));
  expect(runApply).toHaveBeenCalledWith({scope:'project',allowUserScope:false,dryRun:true});
  expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
});
