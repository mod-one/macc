import { useEffect, useState } from 'react';
import { o2Action, type O2Policy, type O2View } from '../../../api/o2';
export default function Policy({ view, saved }: { view: O2View; saved: (view: O2View) => void }) {
  const [draft, setDraft] = useState<O2Policy>(() => { try { const raw = sessionStorage.getItem(`macc.o2.policy:${view.project_root}`); if (raw) return JSON.parse(raw).policy; } catch { /* A storage failure does not block editing. */ } return structuredClone(view.policy); });
  const [revision, setRevision] = useState(() => { try { return JSON.parse(sessionStorage.getItem(`macc.o2.policy:${view.project_root}`) ?? '{}').revision ?? view.revision; } catch { return view.revision; } });
  const [inherit, setInherit] = useState(() => { try { return Boolean(JSON.parse(sessionStorage.getItem(`macc.o2.policy:${view.project_root}`) ?? '{}').inherit); } catch { return false; } }); const [preview, setPreview] = useState<unknown>(null);
  const [filter, setFilter] = useState(''); const [error, setError] = useState(''); const [busy, setBusy] = useState(false); const [notice, setNotice] = useState('');
  const dirty = inherit || JSON.stringify(draft) !== JSON.stringify(view.policy);
  useEffect(() => {
    try { if (dirty) sessionStorage.setItem(`macc.o2.policy:${view.project_root}`, JSON.stringify({ policy: draft, revision, inherit })); else sessionStorage.removeItem(`macc.o2.policy:${view.project_root}`); } catch { /* Draft remains in memory. */ }
    const warn = (event: BeforeUnloadEvent) => { if (dirty) event.preventDefault(); };
    window.addEventListener('beforeunload', warn); return () => window.removeEventListener('beforeunload', warn);
  }, [dirty, draft, revision, inherit, view.project_root]);
  const change = (path: string, value: boolean | number | string) => { const [domain, key] = path.split('.'); setDraft(p => ({ ...p, [domain]: { ...p[domain], [key]: value } })); setInherit(false); setPreview(null); setNotice(''); };
  const run = async (apply: boolean) => { setBusy(true); setError(''); try { if (apply) { const next = await o2Action<O2View>('policy_apply', { revision, policy: inherit ? null : draft }); saved(next); setDraft(structuredClone(next.policy)); setRevision(next.revision); setInherit(false); setPreview(null); setNotice('Policy saved. Changes activate at the next invocation or query. Provider deployment is separate.'); } else setPreview(await o2Action('policy_preview', { revision, policy: inherit ? null : draft })); } catch (e) { setError(String(e)); } finally { setBusy(false); } };
  return <section aria-label="O2 policy">
    <h2>O2 policies</h2><p>Source: {view.source}. Mandatory PRD preservation, evidence review, local storage and task isolation cannot be disabled.</p>
    <label>Find a setting<input value={filter} onChange={e => setFilter(e.target.value)} /></label>
    {view.fields.filter(f => `${f.path} ${f.label}`.toLowerCase().includes(filter.toLowerCase())).map(f => { const [domain, key] = f.path.split('.'); const value = draft[domain]?.[key] ?? f.default; return <div key={f.path} style={{ marginBlock: 12 }}>
      <label htmlFor={`o2-${f.path}`}>{f.label}</label>{' '}
      {f.kind === 'bool' ? <input id={`o2-${f.path}`} type="checkbox" checked={Boolean(value)} onChange={e => change(f.path, e.target.checked)} /> : f.kind === 'enum' ? <select id={`o2-${f.path}`} value={String(value)} onChange={e => change(f.path, e.target.value)}>{f.choices.map(v => <option key={v}>{v}</option>)}</select> : <input id={`o2-${f.path}`} type="number" min={f.min} max={f.max} step={Number.isInteger(f.default) ? 1 : 0.01} value={Number(value)} onChange={e => change(f.path, Number(e.target.value))} />}
      <small style={{ display: 'block' }}><code>{f.path}</code> · default {String(f.default)} · {f.activation}</small>
    </div>; })}
    <button disabled={busy} onClick={() => { void run(false); }}>Validate and preview O2 policy</button>{' '}
    <button disabled={busy || preview === null} onClick={() => { void run(true); }}>Apply O2 policy</button>{' '}
    <button onClick={() => { const defaults: O2Policy = {}; for (const f of view.fields) { const [d, k] = f.path.split('.'); (defaults[d] ??= {})[k] = f.default; } setDraft(defaults); setInherit(false); setPreview(null); }}>Reset O2 draft to defaults</button>{' '}
    <button onClick={() => { setInherit(true); setPreview(null); }}>Remove O2 project overrides</button>{' '}
    <button onClick={() => { setDraft(structuredClone(view.policy)); setRevision(view.revision); setInherit(false); setPreview(null); }}>Discard O2 draft</button>
    {inherit && <p>Preview will remove the four project policy sections and restore defaults.</p>}
    {dirty && <p>Unsaved policy draft.</p>}{notice && <p role="status">{notice}</p>}{error && <p role="alert">{error}</p>}
    {preview !== null && <pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(preview, null, 2)}</pre>}
  </section>;
}
