import { useState } from 'react';
import { type Evidence } from '../../../api/o2';
import { useScopedAction } from './useScopedAction';
interface Sources { providers: Array<{ id: string; languages: string[] }>; tools: string[] }
export default function EvidenceCapture({ path, worktree, identity, captured }: { path: string; worktree: string | null; identity: string; captured: (e: Evidence | null) => void }) {
  const [sources, setSources] = useState<Sources | null>(null);
  const [symbol, setSymbol] = useState(''); const [provider, setProvider] = useState(''); const [tool, setTool] = useState('');
  const [proof, setProof] = useState<Evidence | null>(null);
  const key = JSON.stringify([identity, path, worktree, symbol, provider, tool]);
  const [previousKey, setPreviousKey] = useState(key);
  if (previousKey !== key) { setPreviousKey(key); setProof(null); }
  const { run, busy, error } = useScopedAction(key);
  return <details><summary>Symbol or provider version evidence</summary>
    <p>Capture an exact supported symbol or an explicit registered tool observation. A fingerprint does not prove the proposed claim. Unsupported sources require whole-file evidence.</p>
    <button disabled={busy} onClick={() => { void run(async request => { setSources(await request<Sources>('memory_evidence_sources')); }); }}>Load evidence adapters</button>
    <label>Symbol provider<select value={provider} onChange={e => { setProvider(e.target.value); captured(null); }}><option value="">Choose an adapter</option>{sources?.providers.map(p => <option key={p.id} value={p.id}>{p.id} ({p.languages.join(', ')})</option>)}</select></label>
    <label>Qualified symbol<input value={symbol} onChange={e => { setSymbol(e.target.value); captured(null); }} placeholder="module/Type/method" /></label>
    <button disabled={busy || !path || !symbol || !provider} onClick={() => { void run(async request => { const proof = await request<Evidence>('memory_symbol_evidence', { path, symbol, provider, worktree }); setProof(proof); captured(proof); }); }}>Capture symbol evidence</button>
    <label>Observed tool<select value={tool} onChange={e => { setTool(e.target.value); captured(null); }}><option value="">Choose a registered tool</option>{sources?.tools.map(id => <option key={id} value={id}>{id}</option>)}</select></label>
    <button disabled={busy || !tool} onClick={() => { void run(async request => { const proof = await request<Evidence>('memory_tool_evidence', { tool_id: tool }); setProof(proof); captured(proof); }); }}>Capture provider version evidence</button>
    {proof && <pre role="status">{JSON.stringify(proof, null, 2)}</pre>}{error && <p role="alert">{error}</p>}
  </details>;
}
