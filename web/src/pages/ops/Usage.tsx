import { useEffect, useState } from 'react';
import { getUsage, planAuthority, applyAuthority } from '../../api/sessions';
import type { UsageView, UsageRecord, MigrationPlan } from '../../api/sessions';
const count = (n: number | null) => n === null ? 'Unknown' : n.toLocaleString();
const cost = (n: number | null, currency: string | null, quality: string) => n === null ? 'Unknown' : `${n / 1e9} ${currency ?? 'unknown currency'} (${quality})`;
export default function Usage({ sqlite }: { sqlite: boolean }) {
  const [view, setView] = useState<UsageView | null>(null); const [error, setError] = useState('');
  const [filter, setFilter] = useState(''); const [selected, setSelected] = useState<UsageRecord | null>(null);
  const [plan, setPlan] = useState<MigrationPlan | null>(null); const [busy, setBusy] = useState(false);
  useEffect(() => {
    const controller = new AbortController(); let pending = false;
    const refresh = async () => { if (pending) return; pending = true; try { const next = await getUsage(controller.signal); if (!controller.signal.aborted) setView(next); } catch (e) { if (!controller.signal.aborted) setError(String(e)); } finally { pending = false; } };
    void refresh(); const timer = window.setInterval(() => { void refresh(); }, 5000); return () => { controller.abort(); window.clearInterval(timer); };
  }, []);
  const run = async (action: () => Promise<void>) => { setBusy(true); setError(''); try { await action(); setView(await getUsage()); } catch (e) { setError(String(e)); } finally { setBusy(false); } };
  return <section aria-label="Invocation usage">
    <h2>Invocation usage</h2>
    <p>Known counters are provider reported. Unknown values remain unknown. Cached reads and writes are separate; reasoning is a subset of output. Estimated cost is not an invoice or subscription quota.</p>
    {error && <p role="alert">{error}</p>}
    {sqlite && view && !view.ready && <><p>Usage storage requires a reviewed upgrade while coordinator, supervisor and runners are stopped.</p>
      <button disabled={busy} onClick={() => { void run(async () => { setPlan(await planAuthority('usage-upgrade')); }); }}>Preview usage upgrade</button>
      {plan && <><pre>{JSON.stringify(plan, null, 2)}</pre><button disabled={busy} onClick={() => { void run(async () => { await applyAuthority(plan); setPlan(null); }); }}>Apply reviewed usage upgrade</button></>}
    </>}
    <label htmlFor="usage-filter">Filter usage by task, phase or tool</label>{' '}<input id="usage-filter" value={filter} onChange={e => setFilter(e.target.value)} />
    <h3>Known incremental totals</h3><p>Coverage counts measured invocations per dimension. Missing measurements are excluded from sums. Currency and cost quality remain separate.</p>
    <div style={{ overflowX: 'auto' }}><table><thead><tr>{['Task', 'Tool', 'Phase', 'Calls', 'Uncached input', 'Cache read', 'Cache write', 'Output', 'Cost', 'Coverage'].map(x => <th key={x}>{x}</th>)}</tr></thead><tbody>
      {view?.totals.filter(t => `${t.task_id} ${t.phase} ${t.tool_id}`.toLowerCase().includes(filter.toLowerCase())).map((t, i) => <tr key={i}><td>{t.task_id}</td><td>{t.tool_id}</td><td>{t.phase}</td><td>{t.invocations}</td><td>{count(t.counters.input_uncached)}</td><td>{count(t.counters.cached_read)}</td><td>{count(t.counters.cached_write)}</td><td>{count(t.counters.output)}</td><td>{cost(t.counters.cost, t.currency, t.cost_quality)}</td><td>{`Input ${t.coverage.input_uncached ?? 0}/${t.invocations}; cache read ${t.coverage.cached_read ?? 0}/${t.invocations}; cache write ${t.coverage.cached_write ?? 0}/${t.invocations}; output ${t.coverage.output ?? 0}/${t.invocations}; cost ${t.coverage.cost ?? 0}/${t.invocations}`}</td></tr>)}
    </tbody></table></div>
    <h3>Latest 200 invocation receipts</h3>
    <ul>{view?.records.filter(r => `${r.task_id} ${r.phase} ${r.tool_id}`.toLowerCase().includes(filter.toLowerCase())).map(r => <li key={r.invocation_id}>
      <button onClick={() => setSelected(r)}>{r.task_id} · {r.phase} · {r.tool_id} · {r.status} · {r.token_quality}</button>{' '}{cost(r.incremental.cost, r.currency, r.cost_quality)}
    </li>)}</ul>
    {selected && <><h3>Usage provenance and reported versus incremental counters</h3><pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(selected, null, 2)}</pre></>}
  </section>;
}
