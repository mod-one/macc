import { useCallback, useLayoutEffect, useRef, useState } from 'react';
import { o2Action } from '../../../api/o2';

type Action = (request: typeof o2Action) => Promise<void>;
const obsolete = Symbol('obsolete O2 response');

/** Fence every awaited step, including follow-up writes, to its inspector scope.
 * A newer operation wins; changing scope or unmounting invalidates old closures.
 * Requests already accepted by the backend are not rolled back or replayed.
 */
export function useScopedAction(identity: string) {
  const lifetime = useRef({ identity, sequence: 0, mounted: false });
  const [status, setStatus] = useState({ identity, busy: false, error: '' });
  if (status.identity !== identity) setStatus({ identity, busy: false, error: '' });
  useLayoutEffect(() => {
    const state = lifetime.current;
    state.identity = identity;
    state.mounted = true;
    state.sequence++;
    return () => { state.mounted = false; state.sequence++; };
  }, [identity]);

  const run = useCallback(async (action: Action) => {
    if (!lifetime.current.mounted || lifetime.current.identity !== identity) return;
    const sequence = ++lifetime.current.sequence;
    const current = () => lifetime.current.mounted && lifetime.current.identity === identity && lifetime.current.sequence === sequence;
    setStatus({ identity, busy: true, error: '' });
    const request: typeof o2Action = async <T,>(name: string, fields: Record<string, unknown> = {}, signal?: AbortSignal): Promise<T> => {
      if (!current()) throw obsolete;
      const result = await (signal ? o2Action<T>(name, fields, signal) : o2Action<T>(name, fields));
      if (!current()) throw obsolete;
      return result;
    };
    try { await action(request); }
    catch (error) { if (current() && error !== obsolete) setStatus({ identity, busy: true, error: String(error) }); }
    finally { if (current()) setStatus(previous => ({ ...previous, busy: false })); }
  }, [identity]);

  return { run, busy: status.identity === identity && status.busy, error: status.identity === identity ? status.error : '' };
}
