import { useEffect, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { getO2, o2Action, type O2View } from '../../api/o2';
import type { MigrationPlan } from '../../api/sessions';
import Policy from './o2/Policy';
import Memory from './o2/Memory';
import Context from './o2/Context';
import Providers from './o2/Providers';
import Compaction from './o2/Compaction';
export default function O2() {
  const location = useLocation(); const navigate = useNavigate();
  const isConfig = location.pathname.startsWith('/config/');
  const domain = location.pathname.split('/').at(-1) ?? 'memory';
  const [view, setView] = useState<O2View | null>(null); const [error, setError] = useState(''); const [plan, setPlan] = useState<MigrationPlan | null>(null); const [busy, setBusy] = useState(false);
  useEffect(() => { const controller = new AbortController(); let pending = false; const refresh = async () => { if (pending) return; pending = true; try { const next = await getO2(controller.signal); if (!controller.signal.aborted) setView(next); } catch (e) { if (!controller.signal.aborted) setError(String(e)); } finally { pending = false; } }; void refresh(); const timer = window.setInterval(() => { void refresh(); }, 5000); return () => { controller.abort(); window.clearInterval(timer); }; }, []);
  const upgrade = async (apply: boolean) => { setBusy(true); setError(''); try { if (apply && plan) { await o2Action('upgrade_apply', { source_sha256: plan.source_sha256 }); setPlan(null); setView(await getO2()); } else setPlan(await o2Action<MigrationPlan>('upgrade_plan')); } catch (e) { setError(String(e)); } finally { setBusy(false); } };
  return <main style={{ padding: 24, display: 'grid', gap: 24 }}><h1>O2 shared memory and context</h1>
    <nav aria-label="O2 sections">{[['memory', 'Memory'], ['context', 'Context'], ['compaction', 'Compaction'], ['code-intelligence', 'Code Intelligence'], ['o2', 'Policies']].map(([path, label]) => <button key={path} aria-current={domain === path ? 'page' : undefined} onClick={() => navigate(`${isConfig ? '/config' : '/ops'}/${path}`)}>{label}</button>)}</nav>
    {error && <p role="alert">{error}</p>}
    {view && !view.sqlite && <p>O2 persistence requires SQLite session authority. Open Setup → Sessions to preview the JSON-to-SQLite migration first. Policies can be prepared here.</p>}
    {view?.sqlite && !view.ready && <section><p>O2 schema upgrade requires stopped coordinator, supervisor and runners, and creates a consistent backup.</p><button disabled={busy} onClick={() => { void upgrade(false); }}>Preview O2 upgrade</button>{plan && <><pre>{JSON.stringify(plan, null, 2)}</pre><button disabled={busy} onClick={() => { void upgrade(true); }}>Apply reviewed O2 upgrade</button><button onClick={() => setPlan(null)}>Discard O2 upgrade</button></>}</section>}
    {view && <div hidden={!isConfig && domain !== 'o2'}><Policy view={view} saved={setView} /></div>}
    {view?.ready && <><div hidden={domain !== 'memory'}><Memory /></div><div hidden={domain !== 'context'}><Context /></div><div hidden={domain !== 'compaction'}><Compaction /></div><div hidden={domain !== 'code-intelligence'}><Providers /></div></>}
  </main>;
}
