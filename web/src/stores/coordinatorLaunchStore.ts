import { create } from 'zustand';
import { getCoordinatorLaunchReview } from '../api/client';
import type { CoordinatorLaunchDecision, CoordinatorLaunchReview } from '../api/models';

interface LaunchPrompt {
  open: boolean;
  loading: boolean;
  review: CoordinatorLaunchReview | null;
  error: string | null;
}
export const useCoordinatorLaunchStore = create<LaunchPrompt>(() => ({ open: false, loading: false, review: null, error: null }));
let refreshSequence = 0;
let pending: { resolve: (decision: CoordinatorLaunchDecision) => void; reject: (error: Error) => void } | null = null;

export function requestCoordinatorLaunch(): Promise<CoordinatorLaunchDecision> {
  if (pending) return Promise.reject(new Error('A coordinator launch is already being reviewed.'));
  useCoordinatorLaunchStore.setState({ open: true, loading: true, review: null, error: null });
  const decision = new Promise<CoordinatorLaunchDecision>((resolve, reject) => { pending = { resolve, reject }; });
  void refreshCoordinatorLaunch();
  return decision;
}
export async function refreshCoordinatorLaunch(): Promise<void> {
  useCoordinatorLaunchStore.setState({ loading: true, error: null, review: null });
  const current = pending;
  const sequence = ++refreshSequence;
  try {
    const review = await getCoordinatorLaunchReview();
    if (pending === current && pending && sequence === refreshSequence) useCoordinatorLaunchStore.setState({ review });
  } catch (error) {
    if (pending === current && pending && sequence === refreshSequence) useCoordinatorLaunchStore.setState({ error: error instanceof Error ? error.message : 'Launch review failed.' });
  } finally {
    if (pending === current && pending && sequence === refreshSequence) useCoordinatorLaunchStore.setState({ loading: false });
  }
}
export function closeCoordinatorLaunch(decision?: CoordinatorLaunchDecision): void {
  const request = pending;
  pending = null;
  useCoordinatorLaunchStore.setState({ open: false, loading: false, review: null, error: null });
  if (decision) request?.resolve(decision);
  else request?.reject(new DOMException('Coordinator launch cancelled.', 'AbortError'));
}
