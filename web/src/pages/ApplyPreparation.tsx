import { useState } from 'react';
import type { ApiPreparationInputs, ApiPreparationReport } from '../api/models';
import { o2Action, type O2View } from '../api/o2';
import Policy from './ops/o2/Policy';
import { allowApplyNetwork } from '../api/client';

export default function ApplyPreparation({ report, busy, resume }: {
  report: ApiPreparationReport | null;
  busy: boolean;
  resume: (inputs: ApiPreparationInputs) => Promise<void>;
}) {
  const [suite, setSuite] = useState('');
  const [error, setError] = useState('');
  const [settings, setSettings] = useState<O2View | null>(null);
  if (!report) return null;
  const question = report.question;
  const benchmark = question?.data;
  const result = benchmark?.report as {id?: string; passed?: boolean} | undefined;
  return <section aria-label="Apply preparation assistant" className="rounded-xl border border-[var(--border)] p-4 space-y-3">
    <h2 className="text-lg font-semibold">Preparation · {report.status.replaceAll('_', ' ')}{report.warnings?.length ? ' · with warnings' : ''}</h2>
    {report.current && <p role="status">{report.current}</p>}
    {report.live_output && <details open={report.output_kind === 'interaction'}><summary>{report.output_kind === 'interaction' ? 'Account setup: action required' : 'Technical output'}</summary><pre className="max-h-64 overflow-auto whitespace-pre-wrap" aria-label="Preparation output">{report.live_output}</pre></details>}
    {!!report.warnings?.length && <ul aria-label="Preparation warnings">{report.warnings.map((warning,i)=><li key={i}>WARN {warning}</li>)}</ul>}
    {question?.kind === 'provider_quality' && <div className="flex flex-wrap gap-3">
      <button disabled={busy} onClick={async()=>{try{await o2Action('policy_field_apply',{path:question.data.path,value:question.data.value,revision:report.revision,confirmed:true});await resume({});}catch(e){setError(String(e));}}}>Increase budget to {String(question.data.value)} ms and review rebuild</button>
      <button disabled={busy} onClick={()=>resume({accept_degraded:true})}>Keep partial coverage and review Apply</button>
    </div>}
    <ul>{report.completed.map((step, i) => <li key={i}>✓ {step}</li>)}</ul>
    {report.error && <p role="status">{report.error}</p>}
    {question && <p>{question.message}</p>}
    {question?.kind === 'network_access' && <button disabled={busy} onClick={async () => {
      try {await allowApplyNetwork(report.revision); await resume({});} catch(e) {setError(String(e));}
    }}>Allow project network access and review Apply</button>}
    {question?.kind === 'semantic_suite' && <>
      <label className="block">Labelled benchmark suite (JSON)
        <textarea className="block w-full min-h-40" value={suite} onChange={e => {setSuite(e.target.value); setError('');}} placeholder={JSON.stringify(question.data, null, 2)} />
      </label>
      <button disabled={busy || !suite.trim()} onClick={async () => {
        try {const parsed = JSON.parse(suite); setError(''); await resume({semantic_suite: parsed});}
        catch (e) {setError(String(e));}
      }}>Validate suite and review refreshed Apply plan</button>
    </>}
    {question?.kind === 'semantic_review' && <>
      <pre className="max-h-80 overflow-auto whitespace-pre-wrap">{JSON.stringify(benchmark, null, 2)}</pre>
      <button disabled={busy || benchmark?.approvable !== true || !result?.id || typeof benchmark?.checksum !== 'string'} onClick={() => resume({semantic_approval: {report_id: result!.id!, checksum: benchmark!.checksum as string}})}>Approve these benchmark results and review Apply</button>
    </>}
    {!busy && report.status !== 'ready' && <button onClick={async () => {
      try {setSettings(await o2Action<O2View>('status')); setError('');} catch(e) {setError(String(e));}
    }}>Adjust O2 configuration here</button>}
    {settings && <div aria-label="Apply configuration">
      <Policy view={settings} saved={view => {setSettings(view); setError('Configuration saved. Refresh the Apply plan to continue.');}} />
      <button disabled={busy} onClick={async () => {setSettings(null); await resume({});}}>Continue Apply with saved configuration</button>
    </div>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
