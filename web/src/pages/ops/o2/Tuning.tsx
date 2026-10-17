import { useState } from 'react';
import { useScopedAction } from './useScopedAction';
import type { O2View } from '../../../api/o2';
interface Quality { accepted: boolean; tests_passed: boolean; spec_fidelity: boolean; max_finding_severity: number; retries: number }
interface Trial { id: string; plan: { cases: { task_id: string; revision: string }[] }; [key: string]: unknown }
interface Envelope { checksum: string; report: { id: string; passed: boolean; [key: string]: unknown } }
interface Snapshot { trials?: Trial[]; fields?: string[]; trial?: Trial; result?: Envelope | null; [key: string]: unknown }
interface Preview { report_id: string; checksum: string; revision: string; before: unknown; after: unknown }
const quality = (): Quality => ({ accepted: false, tests_passed: false, spec_fidelity: false, max_finding_severity: 0, retries: 0 });
export default function Tuning({ view }: { view?: O2View }) {
  const [id, setId] = useState(''); const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [fields, setFields] = useState<string[]>([]); const [trials, setTrials] = useState<Trial[]>([]);
  const [field, setField] = useState(''); const [candidate, setCandidate] = useState(''); const [tasks, setTasks] = useState(''); const [revision, setRevision] = useState('');
  const [protocol, setProtocol] = useState(''); const [protocolHash, setProtocolHash] = useState('');
  const [task, setTask] = useState(''); const [baseline, setBaseline] = useState(quality); const [proposed, setProposed] = useState(quality);
  const [evidence, setEvidence] = useState(''); const [evidenceHash, setEvidenceHash] = useState('');
  const [pending, setPending] = useState<{ action: string; fields: Record<string, unknown> } | null>(null);
  const [preview, setPreview] = useState<Preview | null>(null); const [confirmed, setConfirmed] = useState(false); const [output, setOutput] = useState<unknown>(null);
  const [observedRevision, setObservedRevision] = useState(view?.revision);
  if (observedRevision !== view?.revision) { setObservedRevision(view?.revision); setPending(null); setPreview(null); setConfirmed(false); }
  const { run, busy, error } = useScopedAction(`${id}:${view?.revision ?? ''}`);
  const descriptor = view?.fields.find(f => f.path === field);
  const integer = descriptor ? Number.isInteger(Number(descriptor.default)) : false;
  const validCandidate = candidate.trim() !== '' && Number.isFinite(Number(candidate)) && (!integer || Number.isInteger(Number(candidate))) && (!descriptor || (Number(candidate) >= descriptor.min && Number(candidate) <= descriptor.max));
  const invalidate = () => { setPending(null); setConfirmed(false); setPreview(null); };
  const select = (next: string) => { setId(next); setSnapshot(null); setOutput(null); setTask(''); invalidate(); };
  const prepare = (action: string, values: Record<string, unknown>) => { setPending({ action, fields: values }); setConfirmed(false); };
  const inspect = () => { invalidate(); void run(async request => { const value = await request<Snapshot>('tuning_status', { id: id || null }); setSnapshot(value); setOutput(value); if (value.fields) { setFields(value.fields); setField(old => value.fields!.includes(old) ? old : value.fields![0] ?? ''); } if (value.trials) setTrials(value.trials); }); };
  const qualityForm = (label: string, value: Quality, update: (v: Quality) => void) => <fieldset><legend>{label} task quality</legend>
    {(['accepted', 'tests_passed', 'spec_fidelity'] as const).map(key => <label key={key}><input type="checkbox" checked={value[key]} onChange={e => { update({ ...value, [key]: e.target.checked }); invalidate(); }} />{label} {key.replaceAll('_', ' ')}</label>)}
    <label>{label} maximum finding severity<select value={value.max_finding_severity} onChange={e => { update({ ...value, max_finding_severity: Number(e.target.value) }); invalidate(); }}>{['None', 'Low', 'Medium', 'High', 'Critical'].map((name, i) => <option key={name} value={i}>{name}</option>)}</select></label>
    <label>{label} retries<input type="number" min={0} max={1000} step={1} value={value.retries} onChange={e => { update({ ...value, retries: Number(e.target.value) }); invalidate(); }} /></label>
  </fieldset>;
  const report = snapshot?.result;
  return <section><h2>Reviewed policy trials</h2>
    <p>Trials compare one safe policy field on pre-registered independent tasks. Costs come from complete canonical invocation telemetry; estimated or unknown charges cannot authorize adoption. Quality is an explicit operator attestation backed by an inspected file. No task dispatch, installation or policy change occurs implicitly. Enable and configure trials under Context policies.</p>
    <fieldset disabled={busy}><legend>Inspect trials</legend><label>Trial ID<input value={id} onChange={e => select(e.target.value)} /></label>
      <label>Recorded trial<select value={trials.some(t => t.id === id) ? id : ''} onChange={e => select(e.target.value)}><option value="">All trials</option>{trials.map(t => <option key={t.id} value={t.id}>{t.id}</option>)}</select></label>
      <button onClick={inspect}>Inspect policy trials</button></fieldset>
    <details><summary>Pre-register a fixed paired trial</summary><fieldset disabled={busy} onChange={invalidate}>
      <label>Safe policy field<select value={field} onChange={e => setField(e.target.value)}><option value="">Inspect trials to load eligible fields</option>{fields.map(f => <option key={f}>{f}</option>)}</select></label>
      <label>Candidate numeric value<input type="number" min={descriptor?.min} max={descriptor?.max} step={integer ? 1 : "any"} value={candidate} onChange={e => setCandidate(e.target.value)} /></label>
      <label>Representative task IDs<textarea value={tasks} placeholder="Comma-separated distinct task IDs; minimum 20" onChange={e => setTasks(e.target.value)} /></label>
      <label>Full starting Git revision<input value={revision} onChange={e => setRevision(e.target.value)} /></label>
      <label>Trial protocol file<input value={protocol} onChange={e => setProtocol(e.target.value)} /></label><label>Protocol SHA-256<input value={protocolHash} onChange={e => setProtocolHash(e.target.value)} /></label>
      <button disabled={!field || !validCandidate || !tasks.trim() || !revision || !protocol || !protocolHash} onClick={() => prepare('tuning_create', { plan: { field, candidate: Number(candidate), cases: tasks.split(',').map(s => s.trim()).filter(Boolean).map(task_id => ({ task_id, revision })), protocol_path: protocol, protocol_sha256: protocolHash } })}>Prepare trial registration</button>
    </fieldset></details>
    {snapshot?.trial && <><h3>Seal a paired task outcome</h3><p>Run identical task contracts from the registered revision as &lt;pair-id&gt;--baseline and &lt;pair-id&gt;--candidate, each with unused task/checkpoint identities and fresh sessions. Case 1 runs baseline first, case 2 candidate first, alternating thereafter. Use the normal policy preview/apply workflow to select the exact registered arm between stopped runs. Include all failures and correction invocations; do not rerun unfavorable samples.</p>
      <fieldset disabled={busy}><label>Registered task<select value={task} onChange={e => { setTask(e.target.value); setBaseline(quality()); setProposed(quality()); invalidate(); }}><option value="">Select task</option>{snapshot.trial.plan.cases.map(c => <option key={c.task_id}>{c.task_id}</option>)}</select></label>
        {qualityForm('Baseline', baseline, setBaseline)}{qualityForm('Candidate', proposed, setProposed)}
        <label>Task quality evidence file<input value={evidence} onChange={e => { setEvidence(e.target.value); invalidate(); }} /></label><label>Quality evidence SHA-256<input value={evidenceHash} onChange={e => { setEvidenceHash(e.target.value); invalidate(); }} /></label>
        <button disabled={!task || !evidence || !evidenceHash} onClick={() => prepare('tuning_observe', { trial_id: id, observation: { task_id: task, baseline, candidate: proposed, evidence_path: evidence, evidence_sha256: evidenceHash } })}>Prepare sealed quality observation</button>
        <button onClick={() => prepare('tuning_close', { trial_id: id, cancel: false })}>Prepare fixed-sample evaluation</button><button onClick={() => prepare('tuning_close', { trial_id: id, cancel: true })}>Prepare trial cancellation</button>
      </fieldset></>}
    {report && <button disabled={busy || !report.report.passed} onClick={() => { invalidate(); void run(async request => { const value = await request<Preview>('tuning_preview', { report_id: report.report.id, checksum: report.checksum }); setPreview(value); setOutput(value); }); }}>Preview passing policy recommendation</button>}
    {preview && <><pre>{JSON.stringify(preview, null, 2)}</pre><button disabled={busy} onClick={() => prepare('tuning_apply', { report_id: preview.report_id, checksum: preview.checksum, revision: preview.revision })}>Prepare reviewed policy adoption</button></>}
    {pending && <><pre>{JSON.stringify(pending, null, 2)}</pre><label><input type="checkbox" checked={confirmed} onChange={e => setConfirmed(e.target.checked)} />I reviewed this exact operation, controlled trial conditions and evidence</label><button disabled={busy || !confirmed} onClick={() => { const action = pending; setConfirmed(false); void run(async request => { const value = await request(action.action, { ...action.fields, confirmed: true }); setOutput(value); setPending(null); setPreview(null); const next = await request<Snapshot>('tuning_status', { id: id || null }); setSnapshot(next); if (next.trials) setTrials(next.trials); }); }}>Confirm prepared trial operation</button><button disabled={busy} onClick={invalidate}>Discard prepared trial operation</button></>}
    {output !== null && <pre role="status">{JSON.stringify(output, null, 2)}</pre>}{error && <p role="alert">{error}</p>}
  </section>;
}
