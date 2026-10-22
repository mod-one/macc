import { useState } from 'react';
import McpAccessStatus, { accessStates } from './McpAccessStatus';
import { o2Action } from '../../../api/o2';
export default function AgentMemory({ onExtracted }: { onExtracted: () => Promise<void> }) {
  const [checkpoint, setCheckpoint] = useState('');
  const [details, setDetails] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const run = async (fn: () => Promise<void>) => { setBusy(true); setError(''); try { await fn(); } catch (e) { setError(String(e)); } finally { setBusy(false); } };
  const states = accessStates(details);
  return <details><summary>Agent access and phase proposals</summary>
    <p>Configure scoped MCP and phase extraction under Ops → Policies. Agent writes remain proposals. Only the current runner lease authorizes MCP access; promotion and provider operations stay outside its tools.</p>
    <button disabled={busy} onClick={() => { void run(async () => { setDetails(await o2Action('agent_status')); }); }}>Inspect scoped MCP setup</button>
    <label>Extraction checkpoint ID<input value={checkpoint} onChange={e => setCheckpoint(e.target.value)} /></label>
    <button disabled={busy || !checkpoint.trim()} onClick={() => { void run(async () => { setDetails(await o2Action('memory_extract', { checkpoint_id: checkpoint })); await onExtracted(); }); }}>Extract phase proposals</button>
    <p>Extraction reads the recorded outcome and deduplicates unchanged failures. Review candidates in the memory inbox before activation.</p>
    {states !== null && <McpAccessStatus rows={states} />}
    {details !== null && states === null && <pre role="status" style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(details, null, 2)}</pre>}
    {error && <p role="alert">{error}</p>}
  </details>;
}
