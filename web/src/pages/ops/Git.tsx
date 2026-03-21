import React from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import GitGraphView from '../../components/GitGraphView';

interface LocationState {
  from?: string;
}

const Git: React.FC = () => {
  const navigate = useNavigate();
  const location = useLocation();
  const from = (location.state as LocationState | null)?.from;

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <header className="flex items-center justify-between rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4">
        <div>
          <h1 className="text-2xl font-semibold text-[var(--text-primary)]">Git Graph</h1>
          <p className="mt-1 text-sm text-[var(--text-secondary)]">
            Branch topology, merge edges, and task-linked commit visibility.
          </p>
        </div>
        <button
          type="button"
          onClick={() => navigate(from && from !== '/ops/git' ? from : '/dashboard')}
          className="rounded border border-[var(--border)] px-3 py-2 text-sm text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
        >
          Back to panel
        </button>
      </header>

      <div className="min-h-0 flex-1">
        <GitGraphView mode="page" />
      </div>
    </div>
  );
};

export default Git;
