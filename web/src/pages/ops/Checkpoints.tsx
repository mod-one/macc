import { useEffect, useState } from 'react';
import { getCheckpoint, previewReconciliation, applyReconciliation, planAuthority, applyAuthority } from '../../api/sessions';
import type { SessionStoreView, ReconciliationPlan, MigrationPlan } from '../../api/sessions';
export default function Checkpoints({ view, reload, taskScope = '', selectedId = '' }: { view: SessionStoreView; reload: () => Promise<void>; taskScope?: string; selectedId?: string }) {
  const [filter, setFilter] = useState(taskScope);
  const [details, setDetails] = useState<unknown>(null);
  const [error, setError] = useState(''); const [busy, setBusy] = useState(false);
  const [plan, setPlan] = useState<{ id: string; data: ReconciliationPlan } | null>(null);
  const [upgrade, setUpgrade] = useState<MigrationPlan | null>(null);
  useEffect(() => { if (!selectedId) return; let active = true; void getCheckpoint(selectedId).then(value => { if (active) setDetails(value); }).catch(e => { if (active) setError(String(e)); }); return () => { active = false; }; }, [selectedId]);
  useEffect(() => { setFilter(taskScope); }, [taskScope]);
  const run = async (action: () => Promise<void>) => {
    setBusy(true); setError(''); try { await action(); await reload(); } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  return <section aria-label="Task checkpoints">
    <h2>Task checkpoints and session rotation</h2>
    <p>Checkpoints retain task criteria, Git evidence and unresolved findings. They never mark a task delivered. A fresh session must acknowledge its task and phase.</p>
    {error && <p role="alert">{error}</p>}
    {!view.checkpoints_ready && view.backend === 'sqlite' && <>
      <p>Checkpoint storage needs a reviewed upgrade. Stop coordinator, supervisor and runners before applying.</p>
      <button disabled={busy} onClick={() => { void run(async () => { setUpgrade(await planAuthority('upgrade')); }); }}>Preview checkpoint upgrade</button>
      {upgrade && <><pre>{JSON.stringify(upgrade, null, 2)}</pre><button disabled={busy} onClick={() => { void run(async () => { await applyAuthority(upgrade); setUpgrade(null); }); }}>Apply reviewed checkpoint upgrade</button></>}
    </>}
    <label htmlFor="checkpoint-filter">Filter checkpoints by task, phase or role</label>{' '}
    <input id="checkpoint-filter" value={filter} onChange={e => setFilter(e.target.value)} />
    <div style={{ overflowX: 'auto' }}><table><thead><tr>{['Task', 'Phase', 'Role', 'Tool', 'Reason', 'Validation', 'Actions'].map(x => <th key={x}>{x}</th>)}</tr></thead><tbody>
      {view.checkpoints.filter(c => (!taskScope || c.task_id === taskScope) && `${c.task_id} ${c.phase} ${c.role}`.toLowerCase().includes(filter.toLowerCase())).map(c => <tr key={c.id}>
        <td>{c.task_id}</td><td>{c.phase}</td><td>{c.role}</td><td>{c.tool_id}</td><td>{c.reason}</td><td>{c.validation_state}</td><td>
          <button disabled={busy} onClick={() => { void run(async () => { setDetails(await getCheckpoint(c.id)); }); }}>Inspect {c.task_id} checkpoint</button>{' '}
          <button disabled={busy} onClick={() => { void run(async () => { setPlan({ id: c.id, data: await previewReconciliation(c.id) }); }); }}>Preview reconciliation</button>
        </td></tr>)}
    </tbody></table></div>
    {details !== null && <pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(details, null, 2)}</pre>}
    {plan && <div><h3>Review current sources before reconciliation</h3><pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(plan.data, null, 2)}</pre>
      <button disabled={busy} onClick={() => { void run(async () => { await applyReconciliation(plan.id, plan.data.current_sha256); setPlan(null); }); }}>Apply reviewed reconciliation</button>{' '}
      <button onClick={() => setPlan(null)}>Cancel reconciliation</button></div>}
  </section>;
}
