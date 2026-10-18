import { useState } from 'react';
import { ConfirmDialog } from './ConfirmDialog';
import { closeCoordinatorLaunch, refreshCoordinatorLaunch, useCoordinatorLaunchStore } from '../stores/coordinatorLaunchStore';

export function CoordinatorLaunchDialog() {
  const state = useCoordinatorLaunchStore();
  return <LaunchConfirmation key={`${state.open}:${state.review?.revision ?? 'loading'}`} {...state} />;
}
function LaunchConfirmation({ open, loading, review, error }: ReturnType<typeof useCoordinatorLaunchStore.getState>) {
  const [supervisor, setSupervisor] = useState(true);
  const [createFrom, setCreateFrom] = useState('');
  const [allowDirty, setAllowDirty] = useState(false);
  const disabled = loading || !review || review.blockers.length > 0
    || (review.needsBranchCreation && !createFrom) || (review.dirtyReference && !allowDirty);
  return <ConfirmDialog open={open} onOpenChange={(value) => { if (!value) closeCoordinatorLaunch(); }}
    title="Review coordinator launch" description="Review the task source and launch options before starting. Monitoring stays in this Web client."
    confirmLabel="Start coordinator" confirmDisabled={disabled}
    onConfirm={() => { if (review && !disabled) closeCoordinatorLaunch({ confirmed: true, revision: review.revision, supervisor, createFrom: createFrom || null, allowDirtyReference: allowDirty }); }}>
    {loading && <p role="status">Inspecting launch conditions…</p>}
    {error && <p role="alert" className="text-rose-300">{error}</p>}
    {review && <div className="space-y-4">
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm">
        <dt>Task source</dt><dd className="break-all">{review.taskSource}</dd>
        <dt>Reference</dt><dd>{review.referenceBranch}</dd>
        <dt>Tools</dt><dd>{review.tools.join(', ') || 'No tool selected'}</dd>
        <dt>Parallel / dispatch</dt><dd>{review.maxParallel} / {review.maxDispatch}</dd>
      </dl>
      {review.queue.length > 0 && <details><summary>PRD Queue ({review.queue.length})</summary><ol className="list-inside list-decimal">{review.queue.map((path) => <li key={path} className="break-all">{path}</li>)}</ol></details>}
      <label className="flex items-start gap-2"><input type="checkbox" checked={supervisor} onChange={(event) => setSupervisor(event.target.checked)} />Start supervisor to diagnose, repair and resume blocked runs</label>
      {review.needsBranchCreation && <label className="block">Create missing reference branch from
        <select aria-label="Reference branch source" value={createFrom} onChange={(event) => setCreateFrom(event.target.value)} className="mt-1 w-full rounded border bg-[var(--bg-card)] p-2">
          <option value="">Choose a source…</option>{review.branchSources.map((source) => <option key={source} value={source}>{source}</option>)}
        </select></label>}
      {review.dirtyReference && <label className="flex items-start gap-2 text-amber-300"><input type="checkbox" checked={allowDirty} onChange={(event) => setAllowDirty(event.target.checked)} />I reviewed the uncommitted changes and allow this dirty reference once</label>}
      {review.blockers.map((text) => <p role="alert" key={text} className="text-rose-300">{text}</p>)}
      {review.warnings.map((text) => <p key={text} className="text-amber-300">{text}</p>)}
    </div>}
    <button type="button" onClick={() => void refreshCoordinatorLaunch()} disabled={loading} className="mt-3 underline">Refresh review</button>
  </ConfirmDialog>;
}
