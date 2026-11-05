import { useState } from 'react';
import { acceptPrdQueueReview, getPrdQueueReview } from '../api/client';
import type { PrdQueueReview } from '../api/models';

/** Explicit acceptance never launches work; launch conditions are reviewed again. */
export function PrdQueueTransition({ onAccepted }: { onAccepted: () => Promise<void> }) {
  const [review, setReview] = useState<PrdQueueReview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [confirmed, setConfirmed] = useState(false);
  async function inspect() {
    setBusy(true); setError(''); setConfirmed(false);
    try { setReview(await getPrdQueueReview()); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(false); }
  }
  async function accept() {
    if (!review || !confirmed || busy) return;
    setBusy(true); setError('');
    try { await acceptPrdQueueReview(review.revision); await onAccepted(); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); setConfirmed(false); }
    finally { setBusy(false); }
  }
  return <section className="space-y-2 rounded border border-amber-500 p-3" aria-label="PRD queue transition">
    <button type="button" onClick={() => void inspect()} disabled={busy}>Review PRD queue change…</button>
    {review && <>
      <p>Previous series: {review.completed}/{review.previous.length} PRDs completed. Proposed queue: {review.proposed.length} PRDs.</p>
      <ul>{review.changes.map((change) => <li key={change}>{change}</li>)}</ul>
      <p>{review.explanation}</p>
      <label className="flex items-start gap-2"><input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} disabled={busy} />I accept the new queue baseline and returning to its first PRD. Previous progress will be archived; delivered work and worktrees are preserved.</label>
      <button type="button" disabled={!confirmed || busy} onClick={() => void accept()}>Accept queue change</button>
    </>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
