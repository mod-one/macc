import { useState } from 'react';
import { o2Action } from '../../../api/o2';
interface Descriptor { id: string; version: string; origin: string; operations: string[]; local_only: boolean; unsupported: string[] }
interface View { providers: { provider: Descriptor; status: string; diagnostics: string[] }[]; receipts: unknown[]; plans: unknown[] }
interface PlanResult { result: { outcome: string; data?: { id?: string; reason?: string } }; checksum: string | null }
export default function Providers() {
  const [view, setView] = useState<View | null>(null); const [provider, setProvider] = useState('native'); const [operation, setOperation] = useState('deploy'); const [plan, setPlan] = useState<PlanResult | null>(null); const [confirmed, setConfirmed] = useState(false); const [busy, setBusy] = useState(false); const [error, setError] = useState(''); const [details, setDetails] = useState<unknown>(null); const [text, setText] = useState(''); const [kind, setKind] = useState('file_search'); const [worktree, setWorktree] = useState('');
  const run = async (action: () => Promise<void>) => { setBusy(true); setError(''); try { await action(); } catch (e) { setError(String(e)); } finally { setBusy(false); } };
  return <section aria-label="Code Intelligence"><h2>Code Intelligence</h2><p>The native provider ships with MACC. External providers require their own verified adapter. Plans show owned paths and network policy; no package is installed by opening this page.</p>
    <button disabled={busy} onClick={() => { void run(async () => { setView(await o2Action<View>('providers')); }); }}>Refresh provider health</button>
    {view?.providers.map(p => <article key={p.provider.id}><h3>{p.provider.id} · {p.status}</h3><p>{p.provider.version} · {p.provider.origin} · {p.provider.local_only ? 'Local only' : 'Network policy requires review'}</p><p>{p.diagnostics.join('; ')}</p><p>Unavailable: {p.provider.unsupported.join('; ')}</p></article>)}
    <label>Provider ID<input value={provider} onChange={e => { setProvider(e.target.value); setPlan(null); setConfirmed(false); }} /></label>
    <label>Provider operation<select value={operation} onChange={e => { setOperation(e.target.value); setPlan(null); setConfirmed(false); }}>{['install', 'deploy', 'index', 'update', 'rollback', 'disable', 'undeploy', 'uninstall'].map(v => <option key={v}>{v}</option>)}</select></label>
    <button disabled={busy} onClick={() => { void run(async () => { setPlan(await o2Action<PlanResult>('provider_plan', { provider, operation })); setConfirmed(false); }); }}>Preview provider operation</button>
    {plan && <><pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(plan, null, 2)}</pre>{plan.result.outcome === 'supported' && <><label><input type="checkbox" checked={confirmed} onChange={e => setConfirmed(e.target.checked)} />I reviewed the affected paths, origin and network policy</label><button disabled={busy || !confirmed} onClick={() => { void run(async () => { setDetails(await o2Action('provider_apply', { plan_id: plan.result.data?.id, checksum: plan.checksum })); setPlan(null); setConfirmed(false); setView(await o2Action<View>('providers')); }); }}>Apply reviewed provider operation</button></>}</>}
    <details><summary>Deployment receipts and pending plans</summary><pre>{JSON.stringify(view, null, 2)}</pre></details>
    <h3>Query current code</h3><label>Literal code query<input value={text} onChange={e => setText(e.target.value)} /></label>
    <label>Code query type<select value={kind} onChange={e => setKind(e.target.value)}><option>file_search</option><option>references</option></select></label>
    <label>Code worktree (blank uses project)<input value={worktree} onChange={e => setWorktree(e.target.value)} /></label>
    <button disabled={busy} onClick={() => { void run(async () => { setDetails(await o2Action('code_query', { query: { text, kind, worktree: worktree || null } })); }); }}>Search code</button>
    {details !== null && <pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(details, null, 2)}</pre>}{error && <p role="alert">{error}</p>}
  </section>;
}
