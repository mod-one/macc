import { useEffect, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { o2Action } from '../../../api/o2';
export interface TaskOverview {
  task_id: string; sessions: number; lease_records: number; checkpoints: number;
  needs_reconciliation: number; contexts: number; proposals: number;
  latest_context: { id: string; phase: string; optional_estimated_tokens: number; optional_limit_tokens: number; measurement_quality: string } | null;
  latest_checkpoint: { id: string; phase: string; validation_state: string } | null;
}
export interface OverviewView {
  ready: boolean; captured_at: number; notice: string; memory_statuses: Record<string, number>;
  providers: { id: string; state: string; version: string }[]; tasks: TaskOverview[]; next_cursor: string | null;
}
export default function Overview({ task, active, scope }: { task: string; active: boolean; scope: (task: string) => void }) {
  const [snapshot, setSnapshot] = useState<{ key: string; data: OverviewView } | null>(null);
  const [filter, setFilter] = useState(task); const [after, setAfter] = useState<string | null>(null);
  const [error, setError] = useState(''); const [stale, setStale] = useState(false); const [busy, setBusy] = useState(false);
  const snapshotKey = JSON.stringify([task, after]);
  const view = snapshot?.key === snapshotKey ? snapshot.data : null;
  useEffect(() => { setFilter(task); setAfter(null); }, [task]);
  const [refresh, setRefresh] = useState(0); const sequence = useRef(0);
  useEffect(() => {
    if (!active) return;
    const controller = new AbortController(); const current = ++sequence.current; let pending = false; let disposed = false;
    const load = async () => {
      if (pending) return; pending = true; setBusy(true);
      try {
        const next = await o2Action<OverviewView>('overview', { query: { task_id: task || null, after, limit: 25 } }, controller.signal);
        if (!disposed && sequence.current === current) { setSnapshot({ key: JSON.stringify([task, after]), data: next }); setError(''); setStale(false); }
      } catch (e) { if (!disposed && sequence.current === current) { setError(String(e)); setStale(true); } }
      finally { pending = false; if (!disposed && sequence.current === current) setBusy(false); }
    };
    void load(); const timer = window.setInterval(() => { void load(); }, 5000);
    return () => { disposed = true; controller.abort(); window.clearInterval(timer); };
  }, [active, task, after, refresh]);
  const link = (domain: string, id?: string) => `/ops/${domain}?${new URLSearchParams({ task, ...(id ? { id } : {}) })}`;
  return <section aria-label="O2 overview" aria-busy={busy}>
    <h2>Task evidence overview</h2>
    <form onSubmit={e => { e.preventDefault(); setAfter(null); scope(filter.trim()); }}>
      <label>Exact overview task<input value={filter} onChange={e => setFilter(e.target.value)} /></label>
      <button type="submit">Apply task filter</button>
      <button type="button" onClick={() => { setFilter(''); setAfter(null); scope(''); }}>All tasks</button>
    </form>
    <button onClick={() => setRefresh(n => n + 1)}>Refresh overview</button>
    {error && <p role="alert">{error}. Previous snapshot retained; retry with Refresh overview.</p>}
    <p role="status">{view ? `Snapshot ${new Date(view.captured_at * 1000).toLocaleTimeString()} · ${stale ? 'Stale' : 'Observed'}` : 'Waiting for snapshot'}</p>
    {view && <>
      <p>{view.notice}</p>
      <h3>Project memory · stored statuses</h3>
      <dl>{Object.entries(view.memory_statuses).map(([status, count]) => <div key={status}><dt>{status}</dt><dd>{count}</dd></div>)}</dl>
      <h3>Recorded provider deployments</h3>
      {view.providers.length ? <ul>{view.providers.map(p => <li key={p.id}><Link to={`/ops/code-intelligence?${new URLSearchParams({ provider: p.id, task })}`}>{p.id} · {p.state} · {p.version}</Link></li>)}</ul> : <p>No deployment receipts. Inspect Code Intelligence to discover available providers.</p>}
      {view.ready && !view.tasks.length && <p>No evidence records match this task. Check the task ID or wait for its first checkpoint.</p>}
      <div style={{ overflowX: 'auto' }}><table><caption>Recorded task evidence</caption><thead><tr>{['Task', 'Sessions / leases', 'Checkpoints / reconcile', 'Context bundles', 'Proposals', 'Explore'].map(h => <th key={h} scope="col">{h}</th>)}</tr></thead>
        <tbody>{view.tasks.map(t => <tr key={t.task_id}>
          <th scope="row">{t.task_id}</th><td>{t.sessions} / {t.lease_records}</td><td>{t.checkpoints} / {t.needs_reconciliation}</td><td>{t.contexts}</td><td>{t.proposals}</td>
          <td><button onClick={() => { setFilter(t.task_id); setAfter(null); scope(t.task_id); }}>Focus {t.task_id}</button></td>
        </tr>)}</tbody></table></div>
      <button disabled={!after} onClick={() => setAfter(null)}>First task page</button>
      <button disabled={!view.next_cursor} onClick={() => setAfter(view.next_cursor)}>Next task page</button>
      {task && view.tasks.map(t => <section key={t.task_id} aria-label={`Explore ${t.task_id}`}>
        <h3>{t.task_id} evidence</h3>
        <nav aria-label={`Task ${t.task_id} domains`}><Link to={link('memory')}>Memory inbox</Link>{' · '}<Link to={link('context', t.latest_context?.id)}>Context evidence</Link>{' · '}<Link to={link('sessions', t.latest_checkpoint?.id)}>Sessions and checkpoints</Link>{' · '}<Link to={link('compaction')}>Compaction</Link></nav>
        {t.latest_context && <p>Optional context: {t.latest_context.optional_estimated_tokens} / {t.latest_context.optional_limit_tokens} estimated tokens · {t.latest_context.measurement_quality}. Mandatory PRD is separate.</p>}
        {t.latest_checkpoint && <p>Latest checkpoint: {t.latest_checkpoint.phase} · {t.latest_checkpoint.validation_state}. Checkpoints do not prove delivery.</p>}
      </section>)}
      <Link to={`/config/o2?${new URLSearchParams({ task })}`}>Edit effective O2 policies</Link>
    </>}
  </section>;
}
