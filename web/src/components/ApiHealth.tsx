import React from 'react';
import { getHealth } from '../api/client';
import type { ApiHealthResponse } from '../api/models';

const ApiHealth: React.FC = () => {
  const [health, setHealth] = React.useState<ApiHealthResponse | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    const abortController = new AbortController();

    const load = async (): Promise<void> => {
      try {
        const result = await getHealth({ signal: abortController.signal });
        setHealth(result);
        setError(null);
      } catch (err) {
        if (err instanceof DOMException && err.name === 'AbortError') {
          return;
        }
        setError(err instanceof Error ? err.message : 'Failed to fetch health');
      }
    };

    void load();

    return () => {
      abortController.abort();
    };
  }, []);

  return (
    <div className="p-4 border rounded shadow-sm bg-gray-50 mt-4">
      <h2 className="text-lg font-bold mb-2">API Health Status</h2>
      {error ? (
        <p className="text-red-500">Error: {error}</p>
      ) : health ? (
        <p className="text-green-600">
          Status: {health.status}
        </p>
      ) : (
        <p>Loading health...</p>
      )}
    </div>
  );
};

export default ApiHealth;
