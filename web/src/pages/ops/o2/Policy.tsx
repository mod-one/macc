import { useEffect, useRef, useState } from 'react';
import { o2Action, type O2Policy, type O2View } from '../../../api/o2';
export default function Policy({ view, saved }: { view: O2View; saved: (view: O2View) => void }) {
  const [draft, setDraft] = useState<O2Policy>(() => { try { const raw = sessionStorage.getItem(`macc.o2.policy:${view.project_root}`); if (raw) return JSON.parse(raw).policy; } catch { /* A storage failure does not block editing. */ } return structuredClone(view.policy); });
  const [revision, setRevision] = useState(() => { try { return JSON.parse(sessionStorage.getItem(`macc.o2.policy:${view.project_root}`) ?? '{}').revision ?? view.revision; } catch { return view.revision; } });
  const [inherit, setInherit] = useState(() => { try { return Boolean(JSON.parse(sessionStorage.getItem(`macc.o2.policy:${view.project_root}`) ?? '{}').inherit); } catch { return false; } }); const [preview, setPreview] = useState<{ sequence: number; policy: O2Policy | null; revision: string; result: unknown } | null>(null);
  const sequence = useRef(0);
  const invalidate = () => { sequence.current += 1; setPreview(null); };
  const [filter, setFilter] = useState(''); const [error, setError] = useState(''); const [busy, setBusy] = useState(false); const [notice, setNotice] = useState('');
  const dirty = inherit || JSON.stringify(draft) !== JSON.stringify(view.policy);
  useEffect(() => {
    try { if (dirty) sessionStorage.setItem(`macc.o2.policy:${view.project_root}`, JSON.stringify({ policy: draft, revision, inherit })); else sessionStorage.removeItem(`macc.o2.policy:${view.project_root}`); } catch { /* Draft remains in memory. */ }
    const warn = (event: BeforeUnloadEvent) => { if (dirty) event.preventDefault(); };
    window.addEventListener('beforeunload', warn); return () => window.removeEventListener('beforeunload', warn);
  }, [dirty, draft, revision, inherit, view.project_root]);
  const change = (path: string, value: boolean | number | string) => { const [domain, key] = path.split('.'); setDraft(p => ({ ...p, [domain]: { ...p[domain], [key]: value } })); setInherit(false); invalidate(); setNotice(''); };
  const run = async (apply: boolean) => {
    const started = sequence.current;
    const snapshot = apply ? preview : { sequence: started, policy: inherit ? null : structuredClone(draft), revision, result: null };
    if (!snapshot || snapshot.sequence !== started) return;
    setBusy(true); setError('');
    try {
      if (apply) {
        const next = await o2Action<O2View>('policy_apply', { revision: snapshot.revision, policy: snapshot.policy });
        saved(next); setRevision(next.revision); setPreview(null);
        if (sequence.current === started) { setDraft(structuredClone(next.policy)); setInherit(false); }
        setNotice('Reviewed policy saved. Later edits remain an unsaved draft. Changes activate at the next invocation or query.');
      } else {
        const result = await o2Action('policy_preview', { revision: snapshot.revision, policy: snapshot.policy });
        if (sequence.current === started) setPreview({ ...snapshot, result });
      }
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  return <section aria-label="O2 policy">
    <h2>O2 policies</h2><p>Source: {view.source}. Mandatory PRD preservation, evidence review, local storage and task isolation cannot be disabled.</p>
    <label>Find a setting<input value={filter} onChange={e => setFilter(e.target.value)} /></label>
    {view.fields.filter(f => `${f.path} ${f.label}`.toLowerCase().includes(filter.toLowerCase())).map(f => { const [domain, key] = f.path.split('.'); const value = draft[domain]?.[key] ?? f.default; return <div key={f.path} style={{ marginBlock: 12 }}>
      <label htmlFor={`o2-${f.path}`}>{f.label}</label>{' '}
      {f.kind === 'bool' ? <input id={`o2-${f.path}`} type="checkbox" checked={Boolean(value)} onChange={e => change(f.path, e.target.checked)} /> : f.kind === 'enum' ? <select id={`o2-${f.path}`} value={String(value)} onChange={e => change(f.path, e.target.value)}>{f.choices.map(v => <option key={v}>{v}</option>)}</select> : f.kind === 'string' ? <input id={`o2-${f.path}`} type="text" value={String(value)} onChange={e => change(f.path, e.target.value)} /> : <input id={`o2-${f.path}`} type="number" min={f.min} max={f.max} step={Number.isInteger(f.default) ? 1 : 0.01} value={Number(value)} onChange={e => change(f.path, Number(e.target.value))} />}
      <small style={{ display: 'block' }}><code>{f.path}</code> · default {String(f.default)} · {f.activation}</small>
    </div>; })}
    <button disabled={busy} onClick={() => { void run(false); }}>Validate and preview O2 policy</button>{' '}
    <button disabled={busy || preview === null} onClick={() => { void run(true); }}>Apply O2 policy</button>{' '}
    <button onClick={() => { const defaults: O2Policy = {}; for (const f of view.fields) { const [d, k] = f.path.split('.'); (defaults[d] ??= {})[k] = f.default; } setDraft(defaults); setInherit(false); invalidate(); }}>Reset O2 draft to defaults</button>{' '}
    <button onClick={() => { setInherit(true); invalidate(); }}>Remove O2 project overrides</button>{' '}
    <button onClick={() => { setDraft(structuredClone(view.policy)); setRevision(view.revision); setInherit(false); invalidate(); }}>Discard O2 draft</button>
    {inherit && <p>Preview will remove the four project policy sections and restore defaults.</p>}
    {dirty && <p>Unsaved policy draft.</p>}{notice && <p role="status">{notice}</p>}{error && <p role="alert">{error}</p>}
    {preview !== null && <pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(preview.result, null, 2)}</pre>}
  </section>;
}
