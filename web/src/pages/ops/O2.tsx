import { useEffect, useState } from 'react';
import { Link, useLocation, useNavigate } from 'react-router-dom';
import { getO2, o2Action, type O2View } from '../../api/o2';
import type { MigrationPlan } from '../../api/sessions';
import Policy from './o2/Policy';
import Memory from './o2/Memory';
import Context from './o2/Context';
import Providers from './o2/Providers';
import Compaction from './o2/Compaction';
import Overview from './o2/Overview';
import Tuning from './o2/Tuning';
export default function O2() {
  const location = useLocation(); const navigate = useNavigate();
  const isConfig = location.pathname.startsWith('/config/');
  const domain = location.pathname.split('/').at(-1) ?? 'memory';
  const params = new URLSearchParams(location.search); const task = params.get('task') ?? ''; const id = params.get('id') ?? '';
  const scope = (value: string) => { const next = new URLSearchParams(location.search); next.delete('id'); if (value) next.set('task', value); else next.delete('task'); navigate({ pathname: location.pathname, search: next.toString() }); };
  const [view, setView] = useState<O2View | null>(null); const [refreshError, setRefreshError] = useState(''); const [error, setError] = useState(''); const [plan, setPlan] = useState<MigrationPlan | null>(null); const [busy, setBusy] = useState(false);
  useEffect(() => { const controller = new AbortController(); let pending = false; const refresh = async () => { if (pending) return; pending = true; try { const next = await getO2(controller.signal); if (!controller.signal.aborted) { setView(next); setRefreshError(''); } } catch (e) { if (!controller.signal.aborted) setRefreshError(String(e)); } finally { pending = false; } }; void refresh(); const timer = window.setInterval(() => { void refresh(); }, 5000); return () => { controller.abort(); window.clearInterval(timer); }; }, []);
  const upgrade = async (apply: boolean) => { setBusy(true); setError(''); try { if (apply && plan) { await o2Action('upgrade_apply', { source_sha256: plan.source_sha256 }); setPlan(null); setView(await getO2()); } else setPlan(await o2Action<MigrationPlan>('upgrade_plan')); } catch (e) { setError(String(e)); } finally { setBusy(false); } };
  return <main style={{ padding: 24, display: 'grid', gap: 24 }}><h1>O2 shared memory and context</h1>
    <nav aria-label="O2 sections">{[['overview', 'Overview'], ['memory', 'Memory'], ['context', 'Context'], ['compaction', 'Compaction'], ['code-intelligence', 'Code Intelligence'], ['tuning', 'Policy trials'], ['o2', 'Policies']].map(([path, label]) => <button key={path} aria-current={domain === path ? 'page' : undefined} onClick={() => navigate({ pathname: `${isConfig ? '/config' : '/ops'}/${path}`, search: (() => { const next = new URLSearchParams(location.search); if (path !== domain) next.delete('id'); return next.toString(); })() })}>{label}</button>)}</nav>
    <nav aria-label="Related O2 views"><Link to={`${isConfig ? '/ops' : '/config'}/${domain}${location.search}`}>{isConfig ? 'Inspect operational state' : 'Configure policies'}</Link>{' · '}<Link to={`/ops/sessions?${new URLSearchParams({ task })}`}>Sessions and checkpoints</Link></nav>
    {refreshError && <p role="alert">Policy refresh failed; displayed policy may be stale: {refreshError}</p>}
    {error && <p role="alert">{error}</p>}
    {view && !view.sqlite && <p>O2 persistence requires SQLite session authority. Open Setup → Sessions to preview the JSON-to-SQLite migration first. Policies can be prepared here.</p>}
    {view?.sqlite && !view.ready && <section><p>O2 schema upgrade requires stopped coordinator, supervisor and runners, and creates a consistent backup.</p><button disabled={busy} onClick={() => { void upgrade(false); }}>Preview O2 upgrade</button>{plan && <><pre>{JSON.stringify(plan, null, 2)}</pre><button disabled={busy} onClick={() => { void upgrade(true); }}>Apply reviewed O2 upgrade</button><button onClick={() => setPlan(null)}>Discard O2 upgrade</button></>}</section>}
    {view && <div hidden={!isConfig && domain !== 'o2'}><Policy view={view} saved={setView} domainFilter={isConfig ? ({ memory: 'memory', context: 'context_engine', compaction: 'adaptive_compaction', 'code-intelligence': 'code_intelligence', tuning: 'context_engine' } as Record<string, string>)[domain] : undefined} /></div>}
    <div hidden={domain !== 'overview'}><Overview task={task} active={domain === 'overview'} scope={scope} /></div>
    {view?.ready && <>{domain === 'tuning' && <Tuning view={view} />}<div hidden={domain !== 'memory'}><Memory taskScope={task} /></div><div hidden={domain !== 'context'}><Context taskScope={task} selectedId={domain === 'context' ? id : ''} /></div><div hidden={domain !== 'compaction'}><Compaction /></div><div hidden={domain !== 'code-intelligence'}><Providers initialProvider={params.get('provider') ?? undefined} /></div></>}
  </main>;
}
