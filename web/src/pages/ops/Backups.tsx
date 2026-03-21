import React, { useState, useEffect } from 'react';
import { 
  getBackups, 
  restoreBackup 
} from '../../api/client';
import { 
  ApiBackup, 
  ApiBackupRestoreResult 
} from '../../api/models';
import { 
  Button, 
  ConfirmDialog, 
  EmptyState, 
  ErrorBanner, 
  LoadingSpinner, 
  Toast 
} from '../../components';
import { 
  ClockIcon, 
  RefreshIcon, 
  ChevronDownIcon, 
  ChevronUpIcon, 
  AlertTriangleIcon,
  CheckCircleIcon,
  XCircleIcon,
  FolderIcon
} from '../../components/icons';
import { Icons } from '../../components/NavIcons';
import { cn } from '../../components/styles';

const Backups: React.FC = () => {
  const [backups, setBackups] = useState<ApiBackup[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  
  // Restore state
  const [restoreTarget, setRestoreTarget] = useState<ApiBackup | null>(null);
  const [restoring, setRestoring] = useState(false);
  
  // Toast state
  const [toastOpen, setToastOpen] = useState(false);
  const [toastMessage, setToastMessage] = useState<{title: string, desc?: string, variant: 'success' | 'error'}>({
    title: '',
    variant: 'success'
  });

  const fetchBackups = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await getBackups();
      // Sort by timestamp descending
      const sorted = [...data].sort((a, b) => b.timestamp.localeCompare(a.timestamp));
      setBackups(sorted);
    } catch (err: any) {
      setError(err.message || 'Failed to load backups');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchBackups();
  }, []);

  const handleRestore = async () => {
    if (!restoreTarget) return;
    
    setRestoring(true);
    try {
      const result = await restoreBackup(restoreTarget.id, { confirmed: true });
      setToastMessage({
        title: 'Restore successful',
        desc: result.message,
        variant: 'success'
      });
      setToastOpen(true);
      setRestoreTarget(null);
      // Refresh backups as restore created a new one
      fetchBackups();
    } catch (err: any) {
      setToastMessage({
        title: 'Restore failed',
        desc: err.message || 'An unexpected error occurred during restore.',
        variant: 'error'
      });
      setToastOpen(true);
    } finally {
      setRestoring(false);
    }
  };

  const formatTimestamp = (ts: string) => {
    // Format YYYYMMDD-HHMMSS to YYYY-MM-DD HH:MM:SS
    if (ts.length === 15 && ts.includes('-')) {
      const datePart = ts.split('-')[0];
      const timePart = ts.split('-')[1];
      return `${datePart.slice(0, 4)}-${datePart.slice(4, 6)}-${datePart.slice(6, 8)} ${timePart.slice(0, 2)}:${timePart.slice(2, 4)}:${timePart.slice(4, 6)}`;
    }
    return ts;
  };

  const formatSize = (bytes: number) => {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  };

  if (loading && backups.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center py-20">
        <LoadingSpinner size="lg" />
      </div>
    );
  }

  return (
    <section className="mx-auto flex w-full max-w-6xl flex-col gap-6 text-slate-700">
      <header className="rounded-[2rem] border border-slate-200 bg-[radial-gradient(circle_at_top_left,_rgba(56,189,248,0.16),_transparent_38%),linear-gradient(135deg,#ffffff,#f8fafc)] p-6 shadow-sm">
        <div className="flex flex-col gap-5 lg:flex-row lg:items-end lg:justify-between">
          <div className="max-w-2xl">
            <p className="text-sm font-semibold uppercase tracking-[0.2em] text-sky-700">
              System Operations
            </p>
            <h1 className="mb-3 mt-2 text-5xl font-semibold tracking-tight text-slate-950">Backups</h1>
            <p className="max-w-xl text-base leading-7 text-slate-600">
              Browse and restore previous snapshots of your project. MACC automatically creates a 
              backup before any destructive restore or apply operation.
            </p>
          </div>
          
          <Button 
            variant="secondary" 
            onClick={fetchBackups}
            className="flex items-center gap-2"
          >
            <RefreshIcon className={cn("h-4 w-4", loading && "animate-spin")} />
            Refresh
          </Button>
        </div>
      </header>

      {error && <ErrorBanner message={error} onRetry={fetchBackups} />}

      {!loading && backups.length === 0 ? (
        <EmptyState 
          title="No backups found" 
          description="Your backup history is currently empty. Backups will appear here once you perform operations that mutate files."
          icon={<Icons.Archive />}
        />
      ) : (
        <div className="flex flex-col gap-3">
          {backups.map((backup) => (
            <div 
              key={backup.id}
              className={cn(
                "rounded-2xl border border-slate-200 bg-white transition-all overflow-hidden",
                expandedId === backup.id ? "shadow-md ring-1 ring-sky-500/20" : "hover:border-slate-300 shadow-sm"
              )}
            >
              <div className="flex items-center justify-between p-4 sm:p-5">
                <div className="flex flex-1 items-center gap-4 min-w-0">
                  <div className="flex h-12 w-12 flex-shrink-0 items-center justify-center rounded-xl bg-slate-50 text-slate-400">
                    <Icons.Archive />
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <h3 className="text-lg font-bold text-slate-950 truncate">
                        {formatTimestamp(backup.timestamp)}
                      </h3>
                      {backup.userScope && (
                        <span className="rounded-full bg-amber-50 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider text-amber-700 border border-amber-100">
                          User Scope
                        </span>
                      )}
                    </div>
                    <div className="mt-1 flex flex-wrap items-center gap-x-4 gap-y-1 text-sm text-slate-500">
                      <span className="flex items-center gap-1.5">
                        <Icons.FileText />
                        {backup.files} files
                      </span>
                      <span className="flex items-center gap-1.5">
                        <ClockIcon className="h-3.5 w-3.5 text-slate-400" />
                        {formatSize(backup.totalSize)}
                      </span>
                    </div>
                  </div>
                </div>

                <div className="flex items-center gap-3">
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => setExpandedId(expandedId === backup.id ? null : backup.id)}
                    className="hidden sm:flex"
                  >
                    {expandedId === backup.id ? (
                      <>
                        <ChevronUpIcon className="mr-2 h-4 w-4" />
                        Hide Details
                      </>
                    ) : (
                      <>
                        <ChevronDownIcon className="mr-2 h-4 w-4" />
                        View Files
                      </>
                    )}
                  </Button>
                  
                  <Button
                    variant="primary"
                    size="sm"
                    onClick={() => setRestoreTarget(backup)}
                  >
                    Restore
                  </Button>
                </div>
              </div>

              {expandedId === backup.id && (
                <div className="border-t border-slate-100 bg-slate-50/50 p-4 sm:p-5">
                  <h4 className="mb-3 text-xs font-bold uppercase tracking-widest text-slate-500">
                    Backup Contents
                  </h4>
                  <div className="max-h-[300px] overflow-y-auto rounded-xl border border-slate-200 bg-white shadow-inner">
                    <table className="w-full text-left text-sm border-collapse">
                      <thead className="sticky top-0 bg-slate-50 text-xs font-semibold uppercase text-slate-500 border-b border-slate-200">
                        <tr>
                          <th className="px-4 py-2">Path</th>
                          <th className="px-4 py-2 text-right w-24">Size</th>
                        </tr>
                      </thead>
                      <tbody>
                        {backup.entries.map((entry, idx) => (
                          <tr key={idx} className="border-b border-slate-100 last:border-0 hover:bg-slate-50">
                            <td className="px-4 py-2.5 font-mono text-[13px] text-slate-700 break-all">
                              {entry.path}
                            </td>
                            <td className="px-4 py-2.5 text-right font-medium text-slate-500 tabular-nums">
                              {formatSize(entry.size)}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </div>
              )}
            </div>
          ))}
        </div>
      )}

      {/* Confirmation Dialog for Restore */}
      <ConfirmDialog
        open={!!restoreTarget}
        onOpenChange={(open) => !open && setRestoreTarget(null)}
        title="Restore Backup"
        description={`Are you sure you want to restore the backup from ${restoreTarget ? formatTimestamp(restoreTarget.timestamp) : ''}? This will overwrite existing files with the versions from this backup.`}
        confirmLabel={restoring ? "Restoring..." : "Restore Now"}
        intent="danger"
        dangerousConfirmationMode="phrase"
        confirmationPhrase="RESTORE"
        secondaryConfirmationLabel="I understand that this will overwrite my current files and create a new backup."
        onConfirm={handleRestore}
      >
        <div className="mt-4 rounded-xl border border-amber-100 bg-amber-50 p-4">
          <div className="flex gap-3">
            <AlertTriangleIcon className="h-5 w-5 text-amber-600 flex-shrink-0" />
            <div className="text-sm text-amber-900">
              <p className="font-semibold">Safety Note</p>
              <p className="mt-1 opacity-80">
                MACC will create a pre-restore backup of your current files automatically.
                You can revert to your current state if needed.
              </p>
            </div>
          </div>
        </div>
      </ConfirmDialog>

      <Toast 
        open={toastOpen}
        onOpenChange={setToastOpen}
        title={toastMessage.title}
        description={toastMessage.desc}
        variant={toastMessage.variant}
      />
    </section>
  );
};

export default Backups;
