import Usage from './Usage';
import Checkpoints from './Checkpoints';
import { useCallback, useEffect, useState, useRef } from 'react';
import { getSessions, savePolicy, previewPolicy, planAuthority, applyAuthority, sessionAction, getSessionEvents } from '../../api/sessions';
import type { SessionPolicy, SessionStoreView, MigrationPlan } from '../../api/sessions';
import { fieldValue, fieldChange } from './sessionPolicy';
export default function Sessions() {
  const [view, setView] = useState<SessionStoreView | null>(null);
  const [draft, setDraft] = useState<SessionPolicy | null>(null);
  const [revision, setRevision] = useState('');
  const initialized = useRef(false); const [inherit, setInherit] = useState(false);
  const [preview, setPreview] = useState<unknown>(null);
  const [plan, setPlan] = useState<MigrationPlan | null>(null);
  const [storageWarning, setStorageWarning] = useState('');
  const [error, setError] = useState(''); const [message, setMessage] = useState('');
  const [busy, setBusy] = useState(false); const [filter, setFilter] = useState('');
  const [selected, setSelected] = useState<string | null>(null); const [events, setEvents] = useState<unknown[]>([]);
  const [closing, setClosing] = useState<string | null>(null); const [updated, setUpdated] = useState('');
  useEffect(() => {
    const controller = new AbortController(); let pending = false;
    const refresh = async () => {
      if (pending) return; pending = true;
      try {
        const next = await getSessions(controller.signal);
        if (controller.signal.aborted) return;
        setView(next); setUpdated(new Date().toLocaleTimeString());
        if (!initialized.current) {
          initialized.current = true;
          let restored = false;
          try {
            const raw = window.sessionStorage.getItem(`macc.session-policy:${next.project_root}`);
            if (raw) {
              const saved = JSON.parse(raw);
              if (saved.version === 1 && typeof saved.revision === 'string' && next.fields.every(field => Number.isFinite(fieldValue(saved.policy, field.path)))) {
                setDraft(saved.policy); setRevision(saved.revision); setInherit(Boolean(saved.inherit)); restored = true; setMessage('Restored your local policy draft. Review before applying.');
              }
            }
          } catch { setMessage('The previous local draft could not be restored.'); }
          if (!restored) { setDraft(next.policy); setRevision(next.revision); }
        }
      } catch (e) { if (!controller.signal.aborted) setError(String(e)); }
      finally { pending = false; }
    };
    void refresh(); const timer = window.setInterval(() => { void refresh(); }, 5000);
    return () => { controller.abort(); window.clearInterval(timer); };
  }, []);
  const run = useCallback(async (operation: () => Promise<void>) => {
    setBusy(true); setError(''); setMessage('Working…');
    try { await operation(); const next = await getSessions(); setView(next); setUpdated(new Date().toLocaleTimeString()); }
    catch (e) { setError(String(e)); setMessage(''); }
    finally { setBusy(false); }
  }, []);
  useEffect(() => {
    const warn = (event: BeforeUnloadEvent) => {
      if (draft && view && (inherit || JSON.stringify(draft) !== JSON.stringify(view.policy))) event.preventDefault();
    };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [draft, view, inherit]);
  useEffect(() => {
    if (!draft || !view) return;
    const key = `macc.session-policy:${view.project_root}`;
    try {
      if (inherit || JSON.stringify(draft) !== JSON.stringify(view.policy)) window.sessionStorage.setItem(key, JSON.stringify({ version: 1, policy: draft, revision, inherit }));
      else window.sessionStorage.removeItem(key);
    } catch { setStorageWarning('Local draft storage is unavailable. Apply or discard before navigating away.'); }
  }, [draft, view, revision, inherit]);
  const rows = view?.sessions.filter(s => `${s.task_id} ${s.role} ${s.tool_id} ${s.status}`.toLowerCase().includes(filter.toLowerCase())) ?? [];
  return <section aria-label="Session administration" style={{ padding: 24, display: 'grid', gap: 20 }}>
    <h1>Session ownership</h1>
    <p>Authority: <strong>{view?.backend ?? 'Loading…'}</strong> · Updated: {updated || 'waiting'}. Refresh preserves your draft.</p>
    <p>Tasks and roles remain isolated. Interrupted or ambiguous sessions require verified recovery and are never resumed automatically.</p>
    {view && revision && view.revision !== revision && <p role="status">Configuration changed since this draft was loaded. Reload and review before applying.</p>}
    {storageWarning && <p>{storageWarning}</p>}
    {error && <div role="alert">{error} — refresh or review the settings and retry. Your draft is preserved.</div>}
    <div role="status" aria-live="polite">{message}</div>
    {draft && view && <section aria-label="Session policy">
      <h2>Policy</h2><p>Effective source: {view.policy_source}. {inherit ? 'Draft removes the project override.' : ''}</p><p>Changes take effect on the next session claim. Active jobs keep their lease timing.</p>
      {view.fields.map(field => <div key={field.path} style={{ marginBottom: 12 }}>
        <label htmlFor={field.path}>{field.label} ({field.unit})</label>{' '}
        <input id={field.path} type="number" min={field.min} max={field.max} step={1} value={fieldValue(draft, field.path)}
          aria-describedby={`${field.path}-help`} disabled={busy}
          onChange={event => { setDraft(fieldChange(draft, field.path, Number(event.target.value))); setPreview(null); setInherit(false); }} />
        <p id={`${field.path}-help`}>{field.help} Default: {field.default}. Allowed: {field.min}–{field.max}.</p>
      </div>)}
      <button disabled={busy} onClick={() => { void run(async () => { setPreview(await previewPolicy(revision, inherit ? null : draft)); setMessage('Policy validated. Review the diff before Apply.'); }); }}>Validate and preview</button>{' '}
      <button disabled={busy || !preview} onClick={() => { void run(async () => { const saved = await savePolicy(revision, inherit ? null : draft); setRevision(saved.revision); setDraft(saved.policy); setInherit(false); setPreview(null); setMessage('Saved. Effective at next claim.'); }); }}>Apply policy</button>{' '}
      <button disabled={busy} onClick={() => { setDraft(view.policy); setInherit(false); setRevision(view.revision); setPreview(null); setError(''); }}>Discard draft / reload</button>{' '}
      <button disabled={busy} onClick={() => {
        let next = draft; view.fields.forEach(field => { next = fieldChange(next, field.path, field.default); }); setDraft(next); setInherit(false); setPreview(null);
      }}>Stage defaults</button>{' '}
      <button disabled={busy} onClick={() => {
        let next = draft; view.fields.forEach(field => { next = fieldChange(next, field.path, field.default); }); setDraft(next); setInherit(true); setPreview(null);
      }}>Remove project override</button>
      {preview !== null && <pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(preview, null, 2)}</pre>}
    </section>}
    <section aria-label="Storage authority">
      <h2>Storage authority</h2><p>Stop coordinator, supervisor and runners before applying a migration or rollback. Proofs and consistent backups are retained.</p>
      <button disabled={busy} onClick={() => { void run(async () => { setPlan(await planAuthority(view?.backend === 'sqlite' ? 'rollback' : 'migrate')); setMessage('Review the migration plan below.'); }); }}>Preview {view?.backend === 'sqlite' ? 'rollback to JSON' : 'migration to SQLite'}</button>
      {plan && <div><pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(plan, null, 2)}</pre>
        <button disabled={busy} onClick={() => { void run(async () => { const applied = await applyAuthority(plan); setPlan(null); setMessage(`Authority changed. Backup: ${applied.backup ?? 'see migration report'}`); }); }}>Apply reviewed {plan.direction === 'json_to_sqlite' ? 'migration' : 'rollback'}</button></div>}
      <button disabled={busy || view?.backend !== 'sqlite'} onClick={() => { void run(async () => { setMessage(JSON.stringify(await sessionAction('backup'))); }); }}>Create consistent backup</button>{' '}
      <button disabled={busy || view?.backend !== 'sqlite'} onClick={() => { void run(async () => { setMessage(JSON.stringify(await sessionAction('recover'))); }); }}>Quarantine expired dead-owner leases</button>
    </section>
    {view && <Usage sqlite={view.backend === 'sqlite'} />}
    {view && <Checkpoints view={view} reload={async () => { setView(await getSessions()); }} />}
    <section aria-label="Session inventory">
      <h2>Latest sessions</h2>
      <label htmlFor="session-filter">Filter by task, role, tool or status</label>{' '}
      <input id="session-filter" value={filter} onChange={e => setFilter(e.target.value)} />
      <div style={{ overflowX: 'auto' }}><table><thead><tr>{['Task', 'Role', 'Tool', 'Status', 'Generation', 'Uses', 'Owner PID', 'Lease remaining', 'Actions'].map(x => <th key={x}>{x}</th>)}</tr></thead>
        <tbody>{rows.map(s => <tr key={s.id}><td>{s.task_id || 'Unknown legacy owner'}</td><td>{s.role}</td><td>{s.tool_id}</td><td>{s.status}</td><td>{s.generation}</td><td>{s.use_count}</td><td>{s.owner_pid ?? '—'}</td><td>{s.expires_at === null ? '—' : `${Math.max(0, s.expires_at - Math.floor(Date.now() / 1000))}s`}</td>
          <td><button disabled={busy} onClick={() => { void run(async () => { setSelected(s.id); setEvents(view?.backend === 'sqlite' ? await getSessionEvents(s.id) : []); setMessage('Session metadata loaded.'); }); }}>Inspect</button>{' '}
            <button disabled={busy || s.owner_pid !== null || view?.backend !== 'sqlite'} title={s.owner_pid !== null ? 'Stop the lease owner first' : 'Review closure'} onClick={() => setClosing(s.id)}>Close</button></td></tr>)}</tbody></table></div>
      {rows.length === 0 && <p>No matching sessions. JSON metadata becomes inspectable through the migration preview.</p>}
      {closing && <div role="group" aria-label="Confirm session closure"><p>Close logical session {closing}? It will no longer be eligible for resume.</p>
        <button disabled={busy} onClick={() => { void run(async () => { await sessionAction('close', closing); setClosing(null); setMessage('Session closed.'); }); }}>Confirm close</button>{' '}<button onClick={() => setClosing(null)}>Cancel</button></div>}
      {selected && <div><h3>Session {selected}</h3><pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(view?.sessions.find(s => s.id === selected), null, 2)}</pre><h4>Events</h4><pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(events, null, 2)}</pre></div>}
    </section>
  </section>;
}
