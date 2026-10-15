import { useState } from 'react';
import { o2Action } from '../../../api/o2';
interface Bundle { id: string; task_id: string; phase: string; optional_estimated_tokens: number; measurement_quality: string }
export default function Context() {
  const [rows, setRows] = useState<Bundle[]>([]); const [filter, setFilter] = useState(''); const [details, setDetails] = useState<unknown>(null); const [error, setError] = useState(''); const [raw, setRaw] = useState(''); const [kind, setKind] = useState('test');
  const run = async (action: () => Promise<void>) => { setError(''); try { await action(); } catch (e) { setError(String(e)); } };
  return <section aria-label="Context Engine"><h2>Context Engine</h2><p>Mandatory PRD and pinned source references remain separate from optional memory limits. Token budgets here are estimates. Full manifests show selected facts, exclusions and evidence hashes.</p>
    <label>Filter context by task<input value={filter} onChange={e => setFilter(e.target.value)} /></label>
    <button onClick={() => { void run(async () => { setRows(await o2Action<Bundle[]>('context_list')); }); }}>Refresh context bundles</button>
    <ul>{rows.filter(b => b.task_id.toLowerCase().includes(filter.toLowerCase())).map(b => <li key={b.id}><button onClick={() => { void run(async () => { setDetails(await o2Action('context_get', { id: b.id })); }); }}>{b.task_id} · {b.phase} · optional estimate {b.optional_estimated_tokens} tokens</button></li>)}</ul>
    <h3>Reduce tool output</h3><p>This action preserves a private raw artifact and returns a bounded summary. It does not create durable memory or alter provider completion markers.</p>
    <label>Output bundle<select value={kind} onChange={e => setKind(e.target.value)}>{['test', 'lint', 'trace', 'diff', 'log'].map(v => <option key={v}>{v}</option>)}</select></label>
    <label>Raw output<textarea value={raw} onChange={e => setRaw(e.target.value)} /></label>
    <button onClick={() => { void run(async () => { setDetails(await o2Action('reduce', { kind, raw })); }); }}>Reduce and preserve raw evidence</button>
    {details !== null && <pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(details, null, 2)}</pre>}{error && <p role="alert">{error}</p>}
  </section>;
}
