import { PauseIcon, PlayIcon, CopyIcon, DownloadIcon } from './icons';
import { StatusBadge, type StatusTone } from './StatusBadge';
import { cn, interactiveSurfaceClassName, surfaceClassName } from './styles';

export interface StreamTileProps {
  title: string;
  tool: string;
  liveLogTail: string[];
  status: string;
  statusTone?: StatusTone;
  paused?: boolean;
  onPauseToggle?: () => void;
  onCopy?: () => void;
  onDownload?: () => void;
  onClick?: () => void;
  className?: string;
}

export function StreamTile({
  title,
  tool,
  liveLogTail,
  status,
  statusTone = 'active',
  paused = false,
  onPauseToggle,
  onCopy,
  onDownload,
  onClick,
  className,
}: StreamTileProps) {
  return (
    <article 
      className={cn(surfaceClassName, interactiveSurfaceClassName, 'space-y-4 p-5 cursor-pointer', className)}
      onClick={onClick}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-2">
          <div className="flex items-center gap-2">
            <h3 className="text-lg font-semibold">{title}</h3>
            <span className="rounded-full border border-[var(--border)] bg-black/20 px-2.5 py-1 text-xs font-medium text-[var(--text-secondary)]">
              {tool}
            </span>
          </div>
          <StatusBadge status={status} tone={statusTone} />
        </div>
        <div className="flex gap-2" onClick={(e) => e.stopPropagation()}>
          {onPauseToggle && (
            <button
              aria-label={paused ? 'Resume stream' : 'Pause stream'}
              className="inline-flex h-10 w-10 items-center justify-center rounded-md border border-white/10 bg-white/5 transition-colors hover:bg-white/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg-card)]"
              onClick={onPauseToggle}
              type="button"
            >
              {paused ? <PlayIcon className="h-4 w-4" /> : <PauseIcon className="h-4 w-4" />}
            </button>
          )}
          {onCopy && (
            <button
              aria-label="Copy stream log"
              className="inline-flex h-10 w-10 items-center justify-center rounded-md border border-white/10 bg-white/5 transition-colors hover:bg-white/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg-card)]"
              onClick={onCopy}
              type="button"
            >
              <CopyIcon className="h-4 w-4" />
            </button>
          )}
          {onDownload && (
            <button
              aria-label="Download stream segment"
              className="inline-flex h-10 w-10 items-center justify-center rounded-md border border-white/10 bg-white/5 transition-colors hover:bg-white/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg-card)]"
              onClick={onDownload}
              type="button"
            >
              <DownloadIcon className="h-4 w-4" />
            </button>
          )}
        </div>
      </div>
      <div
        aria-label={`${title} live log`}
        className="max-h-44 overflow-auto rounded-lg border border-black/30 bg-black/30 p-4 font-mono text-xs text-[var(--text-secondary)]"
        role="log"
        onWheel={(e) => e.stopPropagation()}
      >
        {liveLogTail.length > 0 ? (
          <div className="space-y-2">
            {liveLogTail.map((line, index) => (
              <p key={`${line}-${index}`} className="break-all">
                {line}
              </p>
            ))}
          </div>
        ) : (
          <p>No output yet.</p>
        )}
      </div>
    </article>
  );
}
