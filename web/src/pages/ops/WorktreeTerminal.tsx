import React from 'react';
import { useParams, Link, useNavigate } from 'react-router-dom';
import { getWorktrees } from '../../api/client';
import type { ApiWorktree } from '../../api/models';
import { TerminalDrawer, type TerminalTarget } from '../../components/TerminalDrawer';
import { Button, LoadingSpinner, ErrorBanner } from '../../components';

const WorktreeTerminal: React.FC = () => {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [worktree, setWorktree] = React.useState<ApiWorktree | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (!id) {
      return;
    }

    let active = true;
    getWorktrees()
      .then((worktrees) => {
        if (!active) return;
        setWorktree(worktrees.find((entry) => entry.id === id) ?? null);
      })
      .catch((err) => {
        if (!active) return;
        setError(err instanceof Error ? err.message : 'Failed to load worktree context.');
      });

    return () => {
      active = false;
    };
  }, [id]);

  const defaultTarget = React.useMemo<TerminalTarget | null>(() => {
    if (!id) {
      return null;
    }
    return {
      terminalType: 'worktree',
      worktreeId: id,
      label: `Worktree: ${worktree?.slug || id}`,
    };
  }, [id, worktree?.slug]);

  if (!id) {
    return <div className="p-8 text-[var(--text-secondary)]">No worktree id provided.</div>;
  }

  return (
    <div className="flex min-h-[70vh] flex-col gap-6">
      <header className="flex items-start justify-between gap-4">
        <div className="space-y-2">
          <p className="text-sm uppercase tracking-[0.24em] text-[var(--text-muted)]">Terminal session</p>
          <h1 className="text-3xl font-bold tracking-tight text-[var(--text-primary)]">
            {worktree?.slug || worktree?.id || `Worktree ${id}`}
          </h1>
          <p className="text-[var(--text-secondary)]">
            This route opens a dedicated terminal drawer for the selected worktree.
          </p>
        </div>
        <Button asChild className="gap-2 h-10 bg-transparent border-white/10 hover:bg-white/10">
          <Link to="/ops/worktrees">Back to Worktrees</Link>
        </Button>
      </header>

      {error && <ErrorBanner message={error} />}
      {!worktree && !error && (
        <div className="flex items-center gap-3 rounded-2xl border border-white/10 bg-[var(--bg-secondary)] px-4 py-3 text-[var(--text-secondary)]">
          <LoadingSpinner size="sm" />
          Loading terminal context...
        </div>
      )}

      <TerminalDrawer
        open
        onOpenChange={(nextOpen) => {
          if (!nextOpen) {
            navigate('/ops/worktrees');
          }
        }}
        defaultTarget={defaultTarget}
        projectRootLabel="Project Root"
      />
    </div>
  );
};

export default WorktreeTerminal;
