import './MemoryTransfer.css';
import { useLayoutEffect, useRef, useState } from 'react';
import { useScopedAction } from './useScopedAction';
import { o2Action } from '../../../api/o2';

type Format = 'jsonl' | 'markdown';
type Plan = { checksum: string; count: number; document?: string; records?: unknown[]; notice: string };
export default function MemoryTransfer({ identity, access, selectedId }: { identity: string; access: Record<string, string | null>; selectedId?: string }) {
  const [mode, setMode] = useState<'export' | 'import'>('export');
  const [format, setFormat] = useState<Format>('jsonl');
  const [ids, setIds] = useState(''); const [document, setDocument] = useState('');
  const [plan, setPlan] = useState<Plan | null>(null); const [confirmed, setConfirmed] = useState(false);
  const [result, setResult] = useState(''); const [fileError, setFileError] = useState('');
  const fileScope = JSON.stringify([identity, access, selectedId, mode, format]);
  const liveFileScope = useRef(fileScope);
  useLayoutEffect(() => {
    liveFileScope.current = fileScope;
    return () => { liveFileScope.current = ''; };
  }, [fileScope]);
  const key = JSON.stringify([identity, access, selectedId, mode, format, ids, document]);
  const [previous, setPrevious] = useState(key);
  if (previous !== key) { setPrevious(key); setPlan(null); setConfirmed(false); setResult(''); }
  const { run, busy, error } = useScopedAction(key);
  const request = mode === 'export'
    ? { ids: (ids || selectedId || '').split(',').map(v => v.trim()).filter(Boolean), access, format }
    : { document, access, format };
  const download = (text: string) => {
    const url = URL.createObjectURL(new Blob([text], { type: format === 'jsonl' ? 'application/x-ndjson' : 'text/markdown' }));
    const link = window.document.createElement('a'); link.href = url; link.download = `macc-memory.${format === 'jsonl' ? 'jsonl' : 'md'}`;
    link.click(); URL.revokeObjectURL(url);
  };
  return <section className="memory-transfer" aria-label="Memory import and export"><h3>Reviewed memory transfer</h3>
    <p>Transfer 1–100 selected memories, up to 2 MiB. Imports create proposals and preserve existing local reviews. Source evidence requires local revalidation.</p>
    <label>Transfer operation<select value={mode} onChange={e => setMode(e.target.value as typeof mode)}><option value="export">Export selected memories</option><option value="import">Import as proposals</option></select></label>
    <label>Transfer format<select value={format} onChange={e => setFormat(e.target.value as Format)}><option value="jsonl">JSONL</option><option value="markdown">MACC Markdown</option></select></label>
    {mode === 'export' ? <label>Memory IDs (comma separated; blank uses selected memory)<input value={ids} onChange={e => setIds(e.target.value)} /></label> : <>
      <label>Memory transfer file<input type="file" accept=".jsonl,.md,.txt" onChange={e => {
        const input = e.currentTarget; const file = input.files?.[0]; setPlan(null); setConfirmed(false); setDocument(''); setFileError('');
        if (!file) return;
        if (file.size > 2 * 1024 * 1024) { setFileError('File exceeds 2 MiB.'); return; }
        void run(async () => { const text = await file.text(); if (input.isConnected && input.files?.[0] === file && liveFileScope.current === fileScope) setDocument(text); });
      }} /></label>
      <label>Transfer document<textarea value={document} onChange={e => setDocument(e.target.value)} /></label>
    </>}
    <button disabled={busy || (mode === 'export' ? !(ids || selectedId) : !document)} onClick={() => { setConfirmed(false); setPlan(null); void run(async call => setPlan(await call<Plan>(`memory_${mode}_preview`, { request }))); }}>Preview memory transfer</button>
    {plan && <><p role="status">{plan.count} records. {plan.notice}</p><pre style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>{JSON.stringify(plan, null, 2)}</pre>
      <label><input type="checkbox" checked={confirmed} onChange={e => setConfirmed(e.target.checked)} />I reviewed the exact records, scopes and source evidence</label>
      <button disabled={busy || !confirmed} onClick={() => { void run(async (call: typeof o2Action) => {
        const value = await call<Plan>(`memory_${mode}`, { request, checksum: plan.checksum, confirmed });
        setConfirmed(false); setPlan(null);
        if (mode === 'export' && value.document) download(value.document);
        setResult(mode === 'export' ? 'Reviewed export downloaded.' : 'Import completed. Inspect each proposal before activation.');
      }); }}>Apply reviewed memory transfer</button>
    </>}
    {result && <p role="status">{result}</p>}{(error || fileError) && <p role="alert">{error || fileError}</p>}
  </section>;
}
