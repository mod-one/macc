import { useState } from 'react';
import { useScopedAction } from './useScopedAction';
interface Case { query: string; ids: string }
interface Envelope { checksum: string; approvable?: boolean; report: { id: string; passed: boolean; [key: string]: unknown } }
export default function Semantic({ identity, access }: { identity: string; access: Record<string, string | null> }) {
  const [cases, setCases] = useState<Case[]>(Array.from({ length: 4 }, () => ({ query: '', ids: '' })));
  const [query, setQuery] = useState(''); const [hits, setHits] = useState<unknown>(null);
  const [topK, setTopK] = useState(3); const [report, setReport] = useState<Envelope | null>(null);
  const [status, setStatus] = useState<unknown>(null); const [confirmed, setConfirmed] = useState(false);
  const [previous, setPrevious] = useState(identity);
  if (previous !== identity) { setPrevious(identity); setReport(null); setStatus(null); setHits(null); setConfirmed(false); }
  const { run, busy, error } = useScopedAction(identity);
  const edit = (i: number, patch: Partial<Case>) => { setCases(old => old.map((c, j) => i === j ? { ...c, ...patch } : c)); setReport(null); setConfirmed(false); };
  return <details><summary>Local semantic search and benchmark gate</summary>
    <p>Corpus-local LSA supplements FTS only after a passing reviewed benchmark. It uses eligible scoped memories, no pretrained model, download or AI tool. Configure limits under Memory policies. Queries and labels are stored in the local audit history.</p>
    <button disabled={busy} onClick={() => { void run(async request => { const value = await request<{ reports: Envelope[] }>('semantic_status', { access }); setStatus(value); setReport(value.reports[0] ?? null); setConfirmed(false); }); }}>Inspect semantic benchmark status</button>
    <label>Approved semantic query<input disabled={busy} value={query} onChange={e => { setQuery(e.target.value); setHits(null); }} /></label>
    <button disabled={busy || !query.trim()} onClick={() => { void run(async request => { setHits(await request('memory_search', { search: { query, access, include_inactive: false } })); }); }}>Search approved facts with local semantics</button>
    {hits !== null && <pre>{JSON.stringify(hits, null, 2)}</pre>}
    {status !== null && <pre>{JSON.stringify(status, null, 2)}</pre>}
    <fieldset disabled={busy}><legend>Representative labelled queries</legend>
      {cases.map((c, i) => <div key={i}><label>Benchmark query {i + 1}<input value={c.query} onChange={e => edit(i, { query: e.target.value })} /></label><label>Relevant memory IDs {i + 1}<input value={c.ids} onChange={e => edit(i, { ids: e.target.value })} placeholder="Comma-separated reviewed memory IDs" /></label></div>)}
      <button disabled={cases.length >= 100} onClick={() => { setCases([...cases, { query: '', ids: '' }]); setReport(null); setConfirmed(false); }}>Add benchmark query</button>
      <label>Benchmark top k<input type="number" min={1} max={10} value={topK} onChange={e => { setTopK(Number(e.target.value)); setReport(null); setConfirmed(false); }} /></label>
      <button disabled={cases.some(c => !c.query.trim() || !c.ids.trim()) || topK < 1 || topK > 10} onClick={() => { void run(async request => { const value = await request<Envelope>('semantic_benchmark', { suite: { access, top_k: topK, cases: cases.map(c => ({ query: c.query, relevant_ids: c.ids.split(',').map(s => s.trim()).filter(Boolean) })) } }); setReport(value); setConfirmed(false); }); }}>Run labelled local benchmark</button>
    </fieldset>
    {report && <><pre role="status">{JSON.stringify(report, null, 2)}</pre>{report.approvable === false && report.report.passed && <p>Run a new benchmark for the current corpus, settings and benchmark recipe before approval.</p>}<label><input type="checkbox" checked={confirmed} onChange={e => setConfirmed(e.target.checked)} />I reviewed the exact corpus, representative labels, quality and resource measurements</label>
      <button disabled={busy || !confirmed || !report.report.passed || report.approvable === false} onClick={() => { void run(async request => { await request('semantic_gate', { report_id: report.report.id, checksum: report.checksum, enabled: true, confirmed }); setConfirmed(false); setStatus(await request('semantic_status', { access })); }); }}>Approve passing semantic benchmark</button>
      <button disabled={busy || !confirmed} onClick={() => { void run(async request => { await request('semantic_gate', { report_id: report.report.id, checksum: report.checksum, enabled: false, confirmed }); setConfirmed(false); setStatus(await request('semantic_status', { access })); }); }}>Revoke semantic gate for this corpus</button></>}
    {error && <p role="alert">{error}</p>}
  </details>;
}
