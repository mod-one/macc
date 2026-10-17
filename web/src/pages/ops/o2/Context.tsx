import { useEffect, useState } from 'react';
import { useScopedAction } from './useScopedAction';
interface Bundle { id: string; task_id: string; phase: string; optional_estimated_tokens: number; measurement_quality: string }
export default function Context({ taskScope = '', selectedId = '' }: { taskScope?: string; selectedId?: string }) {
  const [rows, setRows] = useState<Bundle[]>([]); const [filter, setFilter] = useState(taskScope); const [details, setDetails] = useState<unknown>(null); const [raw, setRaw] = useState(''); const [kind, setKind] = useState('test');
  const [previousTaskScope, setPreviousTaskScope] = useState(taskScope);
  if (previousTaskScope !== taskScope) { setPreviousTaskScope(taskScope); setFilter(taskScope); }
  const identity = JSON.stringify([taskScope, filter, selectedId]);
  const [previousIdentity, setPreviousIdentity] = useState(identity);
  if (previousIdentity !== identity) { setPreviousIdentity(identity); setRows([]); setDetails(null); }
  const { run, busy, error } = useScopedAction(identity);
  useEffect(() => {
    if (!selectedId || filter !== taskScope) return;
    void run(async request => { setDetails(await request('context_get', { id: selectedId })); });
  }, [selectedId, filter, taskScope, run]);
  return <section aria-label="Context Engine"><h2>Context Engine</h2><p>Mandatory PRD and pinned source references remain separate from optional memory limits. Token budgets here are estimates. Full manifests show selected facts, exclusions and evidence hashes.</p>
    <label>Filter context by task<input value={filter} onChange={e => setFilter(e.target.value)} /></label>
    <button disabled={busy} onClick={() => { void run(async request => { setRows(await request<Bundle[]>('context_list')); }); }}>Refresh context bundles</button>
    <ul>{rows.filter(b => b.task_id.toLowerCase().includes(filter.toLowerCase())).map(b => <li key={b.id}><button disabled={busy} onClick={() => { void run(async request => { setDetails(null); setDetails(await request('context_get', { id: b.id })); }); }}>{b.task_id} · {b.phase} · optional estimate {b.optional_estimated_tokens} tokens</button></li>)}</ul>
    <h3>Reduce tool output</h3><p>This action preserves a private raw artifact and returns a bounded summary. It does not create durable memory or alter provider completion markers.</p>
    <label>Output bundle<select value={kind} onChange={e => setKind(e.target.value)}>{['test', 'lint', 'trace', 'diff', 'log'].map(v => <option key={v}>{v}</option>)}</select></label>
    <label>Raw output<textarea value={raw} onChange={e => setRaw(e.target.value)} /></label>
    <button disabled={busy} onClick={() => { void run(async request => { setDetails(await request('reduce', { kind, raw })); }); }}>Reduce and preserve raw evidence</button>
    {details !== null && <pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(details, null, 2)}</pre>}{error && <p role="alert">{error}</p>}
  </section>;
}
