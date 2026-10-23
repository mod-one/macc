import React from 'react';
import { getHooks, hookAction, inspectHook, type HookDefinition, type HooksView } from '../../api/hooks';
import { Button, ConfirmDialog } from '../../components';
import { ApiClientError } from '../../api/client';
function message(error: unknown) {
  return error instanceof ApiClientError ? error.envelope.error.message : error instanceof Error ? error.message : 'Hook operation failed';
}
export default function Hooks() {
  const [view, setView] = React.useState<HooksView>();
  const [query, setQuery] = React.useState('');
  const [error, setError] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  const submitting = React.useRef(false);
  const [pending, setPending] = React.useState<{ body: Record<string, unknown>; title: string; detail: string }>();
  const refresh = async () => {
    setBusy(true);
    try { setView(await getHooks()); setError(''); setPending(undefined); }
    catch (e) { setError(message(e)); }
    finally { setBusy(false); }
  };
  React.useEffect(() => { void refresh(); }, []);
  const change = (id: string, enabled: boolean | null) => {
    setError('');
    setPending({ title: `Update ${id}`, detail: `Activation: ${enabled === null ? 'restore default; feature claims remain active' : enabled ? 'enabled' : 'disabled'}. Run Apply after saving to refresh preparation receipts before launching the coordinator.`, body: { action: 'set', id, enabled, revision: view?.revision } });
  };
  const importFile = async (file?: File) => {
    if (!file || !view) return;
    setBusy(true);
    try {
      if (file.size > 262144) throw new Error('Hook manifest exceeds 256 KiB');
      const review = await inspectHook(JSON.parse(await file.text()) as HookDefinition);
      const h = review.definition;
      setPending({ title: `Import ${h.name}`, detail: `${h.description}\nVersion: ${h.version}\nEvent: ${h.event}\nSkill contracts: ${h.skills.join(', ') || 'explicit invocation only'}\nSource: ${h.source.repository}/${h.source.subpath}\nRules: ${JSON.stringify(h.handler)}\nSHA-256: ${review.sha256}\nStored offline, initially disabled.`, body: { action: 'import', ...review, revision: view.revision } });
      setError('');
    } catch (e) { setError(message(e)); }
    finally { setBusy(false); }
  };
  const confirm = async () => {
    if (!pending) return;
    setBusy(true); submitting.current = true;
    try { setView(await hookAction(pending.body)); setPending(undefined); setError(''); }
    catch (e) { setError(`${message(e)}. Cancel and refresh before reviewing another change.`); }
    finally { submitting.current = false; setBusy(false); }
  };
  return <section className="p-6 space-y-5">
    <header><h1 className="text-2xl font-semibold">Hooks</h1><p>Manage MACC output processors and their feature dependencies.</p></header>
    <p>Enabled means selected in MACC. Native tool installation, local verification and observed execution are separate states. Native shell output requires a supported adapter, Apply, provider trust and an explicit output contract. Internal supervisor evidence hooks need no tool registration.</p>
    {!!view?.native?.length && <section aria-label="Native hook deployments" className="space-y-3"><h2 className="font-semibold">Native tool integration</h2>{view.native.map(n => <article key={n.tool} className="border rounded p-4"><h3>{n.tool}</h3><p>Supported: {n.supported ? 'yes' : 'no'} · Configured: {n.configured ? 'yes' : 'no'} · Installed: {n.installed ? 'yes' : 'no'} · Local verification: {n.locally_verified ? 'passed' : 'pending'} · Execution observed: {n.execution_observed ? 'yes' : 'no'}</p><p>{n.detail}</p><p>Last event reducers: {n.applied.join(', ') || 'none'}</p></article>)}</section>}
    <div className="flex flex-wrap gap-4 items-center">
      <label>Find hooks <input aria-label="Find hooks" value={query} onChange={e => setQuery(e.target.value)} className="border rounded p-2" /></label>
      <Button onClick={() => void refresh()} disabled={busy}>Refresh</Button>
      <label>Import external hook.json <input aria-label="Import external hook.json" type="file" accept=".json,application/json" disabled={busy || !view} onChange={e => { void importFile(e.target.files?.[0]); e.target.value = ''; }} /></label>
    </div>
    {error && !pending && <p role="alert">{error}</p>}
    {!view && <p role="status">Loading hooks…</p>}
    {view && <><p>Tools: {view.tools.join(', ') || 'No tools selected'}. Importing a snapshot does not enable it.</p>
      <div className="space-y-3">{view.hooks.filter(h => `${h.hook.name} ${h.hook.id} ${h.hook.description}`.toLowerCase().includes(query.toLowerCase())).map(h => <article key={h.hook.id} className="border rounded p-4 space-y-2">
        <h2 className="font-semibold">{h.hook.name}</h2><p>{h.hook.description}</p>
        <label><input type="checkbox" aria-label={`Enable ${h.hook.name}`} checked={h.enabled} disabled={busy || h.required_by.length > 0} aria-describedby={`${h.hook.id}-owners`} onChange={e => change(h.hook.id, e.target.checked)} /> Enabled in MACC</label>
        <p id={`${h.hook.id}-owners`}>{h.required_by.length ? `Required by ${h.required_by.join(', ')}. Disable the owning feature or output contract first.` : 'User configurable; no active feature claim.'}</p>
        <p>Version {h.hook.version} · {h.hook.event.replaceAll('_', ' ')} · {h.manual === null ? 'Default selection' : 'Manual selection'}</p>
        <details><summary>Integration and source</summary><p>{h.integration}</p><p>{h.hook.source.repository} / {h.hook.source.subpath} · {h.hook.source.distribution}</p><p>Skill contracts: {h.hook.skills.join(', ') || 'Explicit invocation only'}</p></details>
        {h.manual !== null && <Button disabled={busy} onClick={() => change(h.hook.id, null)}>Use default</Button>}
        {h.hook.source.distribution === 'snapshot' && <Button disabled={busy} onClick={() => setPending({ title: `Remove ${h.hook.name}`, detail: 'Remove this external snapshot and its manual override.', body: { action: 'remove', id: h.hook.id, revision: view.revision } })}>Remove snapshot</Button>}
      </article>)}</div></>}
    <ConfirmDialog open={!!pending} onOpenChange={open => { if (!open && !submitting.current) setPending(undefined); }} title={pending?.title ?? 'Review hook change'} description="Review this hook change before applying it." confirmDisabled={busy} onConfirm={() => void confirm()}><pre className="whitespace-pre-wrap break-words text-sm">{pending?.detail}</pre>{error && <p role="alert">{error}</p>}</ConfirmDialog>
  </section>;
}
