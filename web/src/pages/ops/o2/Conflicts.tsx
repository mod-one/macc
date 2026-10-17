import { useState } from 'react';
import { type MemoryHit } from '../../../api/o2';
import { useScopedAction } from './useScopedAction';
interface Conflict { id: string; left: string; right: string; reason: string; status: string; checksum: string; left_access?: Record<string, string | null>; right_access?: Record<string, string | null> }
export default function Conflicts({ identity, access }: { identity: string; access: Record<string, string | null> }) {
  const [rows, setRows] = useState<Conflict[]>([]); const [selected, setSelected] = useState<Conflict | null>(null);
  const [claims, setClaims] = useState<MemoryHit[]>([]); const [resolution, setResolution] = useState('both_stale'); const [reason, setReason] = useState(''); const [confirmed, setConfirmed] = useState(false);
  const [previousIdentity, setPreviousIdentity] = useState(identity);
  if (previousIdentity !== identity) { setPreviousIdentity(identity); setRows([]); setSelected(null); setClaims([]); setReason(''); setConfirmed(false); }
  const { run, busy, error } = useScopedAction(identity);
  return <section aria-label="Memory conflicts"><h3>Contradictory claims</h3>
    <button disabled={busy} onClick={() => { void run(async request => { setRows(await request<Conflict[]>('memory_conflicts')); setSelected(null); setClaims([]); setConfirmed(false); }); }}>Inspect memory conflicts</button>
    <ul>{rows.map(c => <li key={c.id}>{c.reason} · {c.status}<button disabled={busy} onClick={() => { setSelected(c); setClaims([]); setConfirmed(false); setReason(''); }}>Inspect conflict {c.id}</button></li>)}</ul>
    {selected && <div><p>Left: {selected.left} · Right: {selected.right}. Both claims stay excluded until resolution. Retaining a claim requires it to be active with current evidence.</p>
      <button disabled={busy} onClick={() => { void run(async request => { const left = await request<MemoryHit>('memory_get', { id: selected.left, access: selected.left_access ?? access }); const right = await request<MemoryHit>('memory_get', { id: selected.right, access: selected.right_access ?? access }); setClaims([left, right]); setConfirmed(false); }); }}>Read both claims</button>
      {claims.length > 0 && <pre>{JSON.stringify(claims, null, 2)}</pre>}
      <label>Conflict outcome<select value={resolution} onChange={e => { setResolution(e.target.value); setConfirmed(false); }}>{[['both_stale', 'Mark active claims stale'], ['keep_left', 'Retain left; reject or invalidate right'], ['keep_right', 'Retain right; reject or invalidate left'], ['dismiss', 'Dismiss after verifying compatibility']].map(([v, label]) => <option key={v} value={v}>{label}</option>)}</select></label>
      <label>Conflict resolution reason<input value={reason} onChange={e => { setReason(e.target.value); setConfirmed(false); }} /></label>
      <label><input type="checkbox" checked={confirmed} disabled={claims.length !== 2} onChange={e => setConfirmed(e.target.checked)} />I inspected both claims and their source evidence</label>
      <button disabled={busy || !confirmed || !reason.trim() || selected.status !== 'open'} onClick={() => { void run(async request => { await request('memory_resolve_conflict', { id: selected.id, reason, confirmed, resolution, expected_checksum: selected.checksum }); setRows(await request<Conflict[]>('memory_conflicts')); setSelected(null); setClaims([]); setConfirmed(false); }); }}>Resolve reviewed conflict</button>
    </div>}{error && <p role="alert">{error}</p>}
  </section>;
}
