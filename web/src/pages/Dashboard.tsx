import React from 'react';
import { getStatus } from '../api/client';
import type { ApiCoordinatorStatus } from '../api/models';

const Dashboard: React.FC = () => {
  const [status, setStatus] = React.useState<ApiCoordinatorStatus | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    const abortController = new AbortController();

    const load = async (): Promise<void> => {
      try {
        const nextStatus = await getStatus({ signal: abortController.signal });
        setStatus(nextStatus);
        setError(null);
      } catch (cause) {
        if (
          cause instanceof DOMException &&
          cause.name === 'AbortError'
        ) {
          return;
        }
        setError(cause instanceof Error ? cause.message : 'Failed to load status');
      }
    };

    void load();

    return () => {
      abortController.abort();
    };
  }, []);

  return (
    <div>
      <h1>Dashboard</h1>
      {error ? <p>{error}</p> : null}
      {status ? (
        <p>
          Tasks: {status.total} total, {status.todo} todo, {status.active} active
        </p>
      ) : (
        <p>Loading coordinator status...</p>
      )}
    </div>
  );
};

export default Dashboard;
