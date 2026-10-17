import { useState } from 'react';
import { useScopedAction } from './useScopedAction';
import AgentMemory from './AgentMemory';
import EvidenceCapture from './EvidenceCapture';
import Conflicts from './Conflicts';
import Semantic from './Semantic';
import { o2Action, type Evidence, type MemoryHit } from '../../../api/o2';
export default function Memory({ taskScope = '' }: { taskScope?: string }) {
  const [rows, setRows] = useState<MemoryHit[]>([]); const [query, setQuery] = useState(''); const [task, setTask] = useState(taskScope); const [tool, setTool] = useState(''); const [worktree, setWorktree] = useState('');
  const [feature, setFeature] = useState(''); const [user, setUser] = useState('');
  const [selected, setSelected] = useState<MemoryHit | null>(null); const [notice, setNotice] = useState('');
  const [title, setTitle] = useState(''); const [body, setBody] = useState(''); const [source, setSource] = useState(''); const [kind, setKind] = useState('discovery'); const [scope, setScope] = useState('project'); const [scopeRef, setScopeRef] = useState(''); const [confidence, setConfidence] = useState(0.8);
  const [pull, setPull] = useState<{ next_cursor?: number | null; next_offset?: number | null; id?: string } | null>(null);
  const [replacement, setReplacement] = useState('');
  const [reason, setReason] = useState(''); const [verdict, setVerdict] = useState('active'); const [confirmed, setConfirmed] = useState(false); const [right, setRight] = useState('');
  // Keep proposal drafts; evidence and review authority belong to one exact scope.
  const [previousTaskScope, setPreviousTaskScope] = useState(taskScope);
  if (previousTaskScope !== taskScope) { setPreviousTaskScope(taskScope); setTask(taskScope); }
  const identity = JSON.stringify([taskScope, task, tool, worktree, feature, user, query]);
  const [previousIdentity, setPreviousIdentity] = useState(identity);
  if (previousIdentity !== identity) {
    setPreviousIdentity(identity); setRows([]); setSelected(null); setPull(null); setConfirmed(false);
    setNotice(''); setReplacement(''); setReason(''); setRight('');
  }
  const { run, busy, error } = useScopedAction(identity);
  const [captured, setCaptured] = useState<Evidence | null>(null); const [useCaptured, setUseCaptured] = useState(false);
  const captureKey = JSON.stringify([identity, source, scope, scopeRef]);
  const [previousCaptureKey, setPreviousCaptureKey] = useState(captureKey);
  if (previousCaptureKey !== captureKey) { setPreviousCaptureKey(captureKey); setCaptured(null); setUseCaptured(false); setConfirmed(false); }
  const access = { task_id: task || null, tool_id: tool || null, worktree: worktree || null, feature: feature || null, user_id: user || null };
  const search = async (request: typeof o2Action) => { setRows(await request<MemoryHit[]>('memory_search', { search: { query, access, include_inactive: true } })); };
  return <section aria-label="Memory Ledger">
    <AgentMemory onExtracted={() => run(search)} />
    <h2>Memory Ledger</h2><p>Agent writes are proposals. Only reviewed active facts with current evidence enter automatic context. A memory never completes a task or overrides the PRD.</p>
    <label>Search memories<input value={query} onChange={e => setQuery(e.target.value)} /></label>{' '}
    <label>Task scope<input value={task} onChange={e => setTask(e.target.value)} /></label>{' '}
    <label>Tool scope<input value={tool} onChange={e => setTool(e.target.value)} /></label>{' '}
    <label>Worktree scope<input value={worktree} onChange={e => setWorktree(e.target.value)} /></label>{' '}
    <label>Feature scope<input value={feature} onChange={e => setFeature(e.target.value)} /></label>{' '}
    <label>User scope<input value={user} onChange={e => setUser(e.target.value)} /></label>{' '}
    <button disabled={busy} onClick={() => { void run(search); }}>Search memory inbox</button>{' '}
    <button disabled={busy} onClick={() => { void run(async request => { await request('fts_rebuild'); setNotice('Search projection rebuilt and checked.'); }); }}>Rebuild memory search</button>
    <button disabled={busy} onClick={() => { void run(async request => { setPull(await request('memory_pull', { search: { query, access }, cursor: 0 })); }); }}>Preview bounded agent search</button>
    {pull?.next_cursor != null && <button disabled={busy} onClick={() => { void run(async request => { setPull(await request('memory_pull', { search: { query, access }, cursor: pull.next_cursor })); }); }}>Next agent search page</button>}
    {pull?.next_offset != null && pull.id && <button disabled={busy} onClick={() => { void run(async request => { setPull(await request('memory_read', { id: pull.id, access, offset: pull.next_offset })); }); }}>Next agent record fragment</button>}
    {pull && <pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(pull, null, 2)}</pre>}
    <p>Project facts are always in scope. Feature scopes use task feature_id (or feature). User-scoped memories require explicit operator lookup; they are not automatically injected. Enter matching scope identifiers to inspect restricted facts.</p>
    <ul>{rows.map(hit => <li key={hit.memory.id}><button onClick={() => { void run(async request => { setSelected(null); setConfirmed(false); setPull(null); setSelected(await request<MemoryHit>('memory_get', { id: hit.memory.id, access })); setConfirmed(false); }); }}>{hit.memory.proposal.title} · {hit.memory.status} · {hit.memory.proposal.kind} · {hit.memory.proposal.scope} · confidence {hit.memory.proposal.confidence} · {hit.memory.proposal.evidence.length} evidence</button>{hit.memory.staleness_reason && <p>{hit.memory.staleness_reason}</p>}</li>)}</ul>
    <details><summary>Propose a sourced memory</summary>
      <label>Memory title<input value={title} onChange={e => setTitle(e.target.value)} /></label>
      <label>Memory body<textarea value={body} onChange={e => setBody(e.target.value)} /></label>
      <label>Memory kind<select value={kind} onChange={e => setKind(e.target.value)}>{['constraint', 'discovery', 'decision', 'gotcha', 'api_contract', 'failed_attempt', 'performance_note', 'security_note', 'migration_note', 'provider_note', 'merge_note'].map(v => <option key={v}>{v}</option>)}</select></label>
      <label>Memory scope<select value={scope} onChange={e => setScope(e.target.value)}>{['project', 'feature', 'task', 'worktree', 'tool', 'user'].map(v => <option key={v}>{v}</option>)}</select></label>
      <label>Scope reference<input value={scopeRef} onChange={e => setScopeRef(e.target.value)} /></label>
      <label>Confidence<input type="number" min={0} max={1} step={0.01} value={confidence} onChange={e => setConfidence(Number(e.target.value))} /></label>
      <label>Evidence source path<input value={source} onChange={e => setSource(e.target.value)} /></label>
      <button disabled={busy} onClick={() => { void run(async request => { const evidence = captured ?? await request<Evidence>('memory_evidence', { path: source, worktree: scope === 'worktree' ? scopeRef : null }); await request('memory_propose', { proposal: { kind, title, body, scope, scope_ref: scope === 'project' ? null : scopeRef, confidence, evidence: [evidence], tags: [], source_task: task || null, source_revision: evidence.revision } }); setNotice('Memory proposed. Inspect and review it before activation.'); await search(request); }); }}>Submit memory proposal</button>
      <EvidenceCapture path={source} worktree={scope === 'worktree' ? scopeRef : null} identity={identity} captured={e => { setCaptured(e); setConfirmed(false); setUseCaptured(false); }} />
      {captured && <p role="status">Using captured {captured.kind} evidence: {captured.reference}</p>}
    </details>
    {selected && <section aria-label="Memory review"><h3>{selected.memory.proposal.title}</h3><pre style={{ whiteSpace: 'pre-wrap' }}>{JSON.stringify(selected, null, 2)}</pre>
      <button disabled={busy} onClick={() => { void run(async request => { setPull(await request('memory_read', { id: selected.memory.id, access, offset: 0 })); }); }}>Preview bounded agent record</button>
      {captured && <label><input type="checkbox" checked={useCaptured} onChange={e => { setUseCaptured(e.target.checked); setConfirmed(false); }} />Use captured evidence for revalidation</label>}
      <label>Review verdict<select value={verdict} onChange={e => { setVerdict(e.target.value); setConfirmed(false); }}>{['active', 'rejected', 'stale', 'invalidated'].map(v => <option key={v}>{v}</option>)}</select></label>
      <label>Replacement evidence path (optional revalidation)<input value={replacement} onChange={e => { setReplacement(e.target.value); setConfirmed(false); }} /></label>
      <label>Review reason<input value={reason} onChange={e => { setReason(e.target.value); setConfirmed(false); }} /></label>
      <label><input type="checkbox" checked={confirmed} onChange={e => setConfirmed(e.target.checked)} />I reviewed the source evidence and this transition</label>
      <button disabled={busy || !confirmed} onClick={() => { void run(async request => { const evidence = useCaptured && captured ? [captured] : replacement ? [await request<Evidence>('memory_evidence', { path: replacement, worktree: selected.memory.proposal.scope === 'worktree' ? selected.memory.proposal.scope_ref : null })] : null; await request('memory_review', { review: { id: selected.memory.id, status: verdict, reason, confirmed, expected_checksum: selected.checksum, evidence } }); setSelected(null); setConfirmed(false); await search(request); }); }}>Apply reviewed memory transition</button>
      <label>Conflicting memory ID<input value={right} onChange={e => setRight(e.target.value)} /></label>
      <button disabled={busy || !confirmed} onClick={() => { void run(async request => { await request('memory_conflict', { left: selected.memory.id, right, reason }); setNotice('Conflict recorded; both claims are excluded from automatic context.'); }); }}>Record reviewed conflict</button>
    </section>}
    <Semantic identity={identity} access={access} />
    <Conflicts identity={identity} access={access} />
    {notice && <p role="status">{notice}</p>}{error && <p role="alert">{error}</p>}
  </section>;
}
