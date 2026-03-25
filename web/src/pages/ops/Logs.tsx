import React from 'react';
import type { ApiEventStreamMessage, ApiLogFile, ApiLogContent } from '../../api/models';
import { getLogs, getLogContent } from '../../api/client';
import { buildUrl } from '../../api/client';
import { useEventSource } from '../../hooks/useEventSource';
import { LoadingSpinner } from '../../components/LoadingSpinner';
import { cn } from '../../components/styles';

/* ------------------------------------------------------------------ */
/*  Constants                                                          */
/* ------------------------------------------------------------------ */

const EVENT_LIMIT = 250;
const LOG_PAGE_SIZE = 500;

type TabId = 'stream' | 'browser' | 'events';

/* ------------------------------------------------------------------ */
/*  Shared helpers (kept from original)                                */
/* ------------------------------------------------------------------ */

function connectionTone(state: 'connecting' | 'open' | 'closed'): string {
  if (state === 'open') return 'border-emerald-200 bg-emerald-50 text-emerald-900';
  if (state === 'closed') return 'border-rose-200 bg-rose-50 text-rose-900';
  return 'border-amber-200 bg-amber-50 text-amber-900';
}

function formatTimestamp(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toISOString().replace('T', ' ').replace('.000Z', 'Z');
}

function summarizeEvent(event: ApiEventStreamMessage): string {
  const { payload } = event;
  const fragments = [payload.msg, payload.detail, payload.command, payload.event, payload.state];
  const summary = fragments.find((f) => typeof f === 'string' && f.length > 0);
  return typeof summary === 'string' ? summary : 'Coordinator event received.';
}

function eventDetails(event: ApiEventStreamMessage): string | null {
  const details: Record<string, unknown> = {};
  for (const key of ['task_id', 'phase', 'status', 'source', 'event_id', 'seq'] as const) {
    const value = event.payload[key];
    if (value !== undefined) details[key] = value;
  }
  if (Object.keys(details).length === 0 && event.payload.payload === undefined) return null;
  return JSON.stringify(
    event.payload.payload === undefined ? details : { ...details, payload: event.payload.payload },
    null,
    2,
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function categoryFromPath(path: string): string {
  const parts = path.split('/');
  if (parts.length >= 2) return parts[0]!;
  return 'other';
}

/* ------------------------------------------------------------------ */
/*  Structured event parsing for JSONL                                 */
/* ------------------------------------------------------------------ */

interface StructuredEvent {
  line: number;
  raw: string;
  parsed: Record<string, unknown> | null;
}

function parseJsonlLines(lines: string[]): StructuredEvent[] {
  return lines.map((raw, i) => {
    try {
      const parsed = JSON.parse(raw);
      if (typeof parsed === 'object' && parsed !== null) {
        return { line: i + 1, raw, parsed: parsed as Record<string, unknown> };
      }
    } catch {
      /* not JSON */
    }
    return { line: i + 1, raw, parsed: null };
  });
}

/* ------------------------------------------------------------------ */
/*  Tab button                                                         */
/* ------------------------------------------------------------------ */

function TabButton({
  active,
  label,
  onClick,
}: {
  active: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        'rounded-xl px-4 py-2 text-sm font-semibold transition-colors',
        active
          ? 'bg-sky-600 text-white shadow-sm'
          : 'bg-white text-slate-600 hover:bg-slate-100',
      )}
      onClick={onClick}
    >
      {label}
    </button>
  );
}

/* ================================================================== */
/*  Live Stream tab (preserved from original)                          */
/* ================================================================== */

function LiveStreamTab() {
  const { connectionState, events, replayGapDetected, reconnectAttempt } = useEventSource(
    '/events',
    { maxEvents: EVENT_LIMIT },
  );
  const [selectedType, setSelectedType] = React.useState<string>('all');

  const eventTypes = Array.from(new Set(events.map((e) => e.payload.type))).sort();
  const filteredEvents =
    selectedType === 'all' ? events : events.filter((e) => e.payload.type === selectedType);

  return (
    <>
      <div className="mb-4 flex flex-wrap items-center gap-4">
        <div className="grid gap-3 rounded-2xl border border-slate-200 bg-white/90 p-4 text-sm text-slate-600 shadow-sm sm:grid-cols-3">
          <div>
            <p className="font-medium uppercase tracking-[0.16em] text-slate-500">Connection</p>
            <div
              className={`mt-2 inline-flex rounded-full border px-3 py-1 text-sm font-semibold capitalize ${connectionTone(connectionState)}`}
            >
              {connectionState}
            </div>
          </div>
          <div>
            <p className="font-medium uppercase tracking-[0.16em] text-slate-500">Buffered</p>
            <p className="mt-2 text-2xl font-semibold text-slate-950">{events.length}</p>
          </div>
          <div>
            <label
              className="font-medium uppercase tracking-[0.16em] text-slate-500"
              htmlFor="log-type-filter"
            >
              Event type
            </label>
            <select
              id="log-type-filter"
              className="mt-2 w-full rounded-xl border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 shadow-sm outline-none transition focus:border-sky-500"
              value={selectedType}
              onChange={(e) => setSelectedType(e.target.value)}
            >
              <option value="all">All events</option>
              {eventTypes.map((t) => (
                <option key={t} value={t}>
                  {t}
                </option>
              ))}
            </select>
          </div>
        </div>
      </div>

      <section className="rounded-[2rem] border border-slate-200 bg-white p-4 shadow-sm">
        <div className="mb-4 flex items-center justify-between gap-3 px-2">
          <div>
            <h2 className="text-2xl font-semibold text-slate-950">Live feed</h2>
            <p className="text-sm leading-6 text-slate-500">
              Heartbeats and coordinator events arrive over the same stream.
            </p>
            {connectionState === 'connecting' && reconnectAttempt > 0 ? (
              <p className="mt-2 text-sm font-medium text-amber-700">
                Reconnecting to the event stream (attempt {reconnectAttempt}).
              </p>
            ) : null}
            {replayGapDetected ? (
              <p className="mt-2 text-sm font-medium text-amber-700">
                Stream resumed live without replay support. Some earlier events may be missing.
              </p>
            ) : null}
          </div>
          <p className="text-sm text-slate-500">
            Showing {filteredEvents.length} of {events.length}
          </p>
        </div>

        <div className="h-[32rem] overflow-y-auto rounded-[1.5rem] border border-slate-200 bg-slate-950/95 p-3">
          {filteredEvents.length === 0 ? (
            <div className="flex h-full items-center justify-center rounded-[1.2rem] border border-dashed border-slate-700 bg-slate-950 px-6 text-center text-sm text-slate-400">
              {events.length === 0
                ? 'Waiting for the first event from /api/v1/events.'
                : 'No events match the selected type.'}
            </div>
          ) : (
            <div className="flex flex-col gap-3">
              {filteredEvents.map((event) => {
                const details = eventDetails(event);
                return (
                  <article
                    key={`${event.payload.event_id}-${event.stream}-${event.receivedAt}`}
                    className="rounded-[1.2rem] border border-slate-800 bg-slate-900 p-4 text-slate-100 shadow-sm"
                  >
                    <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                      <div>
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="rounded-full bg-sky-500/15 px-2.5 py-1 text-xs font-semibold uppercase tracking-[0.14em] text-sky-200">
                            {event.payload.type}
                          </span>
                          <span className="rounded-full bg-slate-800 px-2.5 py-1 text-xs font-medium uppercase tracking-[0.14em] text-slate-300">
                            {event.stream}
                          </span>
                          <span className="rounded-full bg-slate-800 px-2.5 py-1 text-xs font-medium uppercase tracking-[0.14em] text-slate-300">
                            {String(event.payload.status)}
                          </span>
                        </div>
                        <p className="mt-3 text-sm leading-6 text-slate-200">
                          {summarizeEvent(event)}
                        </p>
                      </div>
                      <div className="text-xs uppercase tracking-[0.16em] text-slate-400 lg:text-right">
                        <p>{formatTimestamp(event.payload.ts)}</p>
                        <p className="mt-1">seq {event.payload.seq}</p>
                      </div>
                    </div>

                    <div className="mt-4 grid gap-2 text-sm text-slate-300 sm:grid-cols-2 xl:grid-cols-4">
                      <p>
                        <span className="text-slate-500">Source:</span> {event.payload.source}
                      </p>
                      <p>
                        <span className="text-slate-500">Task:</span>{' '}
                        {typeof event.payload.task_id === 'string' ? event.payload.task_id : 'n/a'}
                      </p>
                      <p>
                        <span className="text-slate-500">Phase:</span>{' '}
                        {typeof event.payload.phase === 'string' ? event.payload.phase : 'n/a'}
                      </p>
                      <p>
                        <span className="text-slate-500">Event ID:</span>{' '}
                        {event.payload.event_id}
                      </p>
                    </div>

                    {details ? (
                      <pre className="mt-4 overflow-x-auto rounded-xl border border-slate-800 bg-slate-950 px-3 py-3 text-xs leading-6 text-slate-300">
                        {details}
                      </pre>
                    ) : null}
                  </article>
                );
              })}
            </div>
          )}
        </div>
      </section>
    </>
  );
}

/* ================================================================== */
/*  File Browser tab                                                   */
/* ================================================================== */

function FileBrowserTab() {
  const [files, setFiles] = React.useState<ApiLogFile[]>([]);
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState<string | null>(null);
  const [selectedFile, setSelectedFile] = React.useState<string | null>(null);
  const [content, setContent] = React.useState<ApiLogContent | null>(null);
  const [contentLoading, setContentLoading] = React.useState(false);
  const [contentError, setContentError] = React.useState<string | null>(null);
  const [searchTerm, setSearchTerm] = React.useState('');
  const [timestampJump, setTimestampJump] = React.useState('');
  const [tailMode, setTailMode] = React.useState(false);
  const [offset, setOffset] = React.useState(0);
  const contentRef = React.useRef<HTMLDivElement>(null);
  const tailIntervalRef = React.useRef<ReturnType<typeof setInterval> | null>(null);
  const contentTotalRef = React.useRef(0);
  React.useEffect(() => {
    contentTotalRef.current = content?.total ?? 0;
  }, [content?.total]);

  // Fetch file list
  React.useEffect(() => {
    let cancelled = false;
    setLoading(true);
    getLogs()
      .then((result) => {
        if (!cancelled) {
          setFiles(result);
          setError(null);
        }
      })
      .catch((err) => {
        if (!cancelled) setError(err instanceof Error ? err.message : 'Failed to load log files');
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Fetch file content when selected
  const fetchContent = React.useCallback(
    (path: string, pageOffset: number, search?: string) => {
      setContentLoading(true);
      setContentError(null);
      getLogContent(path, { offset: pageOffset, limit: LOG_PAGE_SIZE, search: search || undefined })
        .then((result) => {
          setContent(result);
          setOffset(pageOffset);
        })
        .catch((err) => {
          setContentError(err instanceof Error ? err.message : 'Failed to load file');
        })
        .finally(() => {
          setContentLoading(false);
        });
    },
    [],
  );

  React.useEffect(() => {
    if (selectedFile) {
      setTailMode(false);
      setSearchTerm('');
      setTimestampJump('');
      fetchContent(selectedFile, 0);
    } else {
      setContent(null);
    }
  }, [selectedFile, fetchContent]);

  // Tail mode: poll for latest content
  React.useEffect(() => {
    if (tailMode && selectedFile && contentTotalRef.current > 0) {
      tailIntervalRef.current = setInterval(() => {
        const tailOffset = Math.max(0, contentTotalRef.current - LOG_PAGE_SIZE);
        getLogContent(selectedFile, { offset: tailOffset, limit: LOG_PAGE_SIZE })
          .then((result) => {
            setContent(result);
            setOffset(tailOffset);
            if (contentRef.current) {
              contentRef.current.scrollTop = contentRef.current.scrollHeight;
            }
          })
          .catch(() => {
            /* silently retry on next interval */
          });
      }, 2000);
    }
    return () => {
      if (tailIntervalRef.current) {
        clearInterval(tailIntervalRef.current);
        tailIntervalRef.current = null;
      }
    };
  }, [tailMode, selectedFile]);

  // Group files by category
  const grouped = React.useMemo(() => {
    const map = new Map<string, ApiLogFile[]>();
    for (const file of files) {
      const cat = categoryFromPath(file.path);
      const list = map.get(cat) ?? [];
      list.push(file);
      map.set(cat, list);
    }
    return map;
  }, [files]);

  // Search handler
  const handleSearch = () => {
    if (selectedFile) {
      fetchContent(selectedFile, 0, searchTerm);
    }
  };

  // Timestamp jump
  const handleTimestampJump = () => {
    if (!content || !timestampJump) return;
    const target = timestampJump.toLowerCase();
    const idx = content.lines.findIndex((line) => line.toLowerCase().includes(target));
    if (idx >= 0 && contentRef.current) {
      const lineElements = contentRef.current.querySelectorAll('[data-line-index]');
      const el = lineElements[idx];
      if (el) {
        el.scrollIntoView({ behavior: 'smooth', block: 'center' });
        (el as HTMLElement).classList.add('bg-amber-900/40');
        setTimeout(() => (el as HTMLElement).classList.remove('bg-amber-900/40'), 2000);
      }
    }
  };

  // Download handler
  const handleDownload = () => {
    if (!selectedFile) return;
    const url = buildUrl(`/logs/${encodeURIComponent(selectedFile)}`);
    const params = new URLSearchParams({ offset: '0', limit: '999999' });
    const anchor = document.createElement('a');
    anchor.href = `${url}?${params.toString()}`;
    anchor.download = selectedFile.split('/').pop() || 'log.txt';
    anchor.click();
  };

  // Pagination
  const totalPages = content ? Math.ceil(content.total / LOG_PAGE_SIZE) : 0;
  const currentPage = Math.floor(offset / LOG_PAGE_SIZE) + 1;

  // Filtered lines (client-side filtering for loaded content when no server search)
  const displayLines = React.useMemo(() => {
    if (!content) return [];
    if (!searchTerm) return content.lines;
    const lower = searchTerm.toLowerCase();
    return content.lines.filter((line) => line.toLowerCase().includes(lower));
  }, [content, searchTerm]);

  return (
    <div className="flex gap-4" style={{ minHeight: '36rem' }}>
      {/* Sidebar: file browser */}
      <aside className="w-72 shrink-0 overflow-y-auto rounded-2xl border border-slate-200 bg-white p-3 shadow-sm">
        <h3 className="mb-3 text-sm font-semibold uppercase tracking-[0.16em] text-slate-500">
          Log files
        </h3>
        {loading ? (
          <div className="flex justify-center py-6">
            <LoadingSpinner label="Loading files" />
          </div>
        ) : error ? (
          <p className="text-sm text-rose-600">{error}</p>
        ) : files.length === 0 ? (
          <p className="text-sm text-slate-400">No log files found.</p>
        ) : (
          Array.from(grouped.entries())
            .sort(([a], [b]) => a.localeCompare(b))
            .map(([category, catFiles]) => (
              <div key={category} className="mb-3">
                <p className="mb-1 text-xs font-bold uppercase tracking-[0.14em] text-slate-400">
                  {category}
                </p>
                {catFiles.map((file) => (
                  <button
                    key={file.path}
                    type="button"
                    className={cn(
                      'mb-0.5 w-full rounded-lg px-2 py-1.5 text-left text-xs transition-colors',
                      selectedFile === file.path
                        ? 'bg-sky-100 font-semibold text-sky-800'
                        : 'text-slate-700 hover:bg-slate-100',
                    )}
                    onClick={() => setSelectedFile(file.path)}
                    title={`${file.path} (${formatBytes(file.size)})`}
                  >
                    <span className="block truncate">{file.path.split('/').pop()}</span>
                    <span className="text-[10px] text-slate-400">
                      {formatBytes(file.size)}
                      {file.modified ? ` \u00b7 ${formatTimestamp(file.modified)}` : ''}
                    </span>
                  </button>
                ))}
              </div>
            ))
        )}
      </aside>

      {/* Main content area */}
      <div className="flex min-w-0 flex-1 flex-col gap-3">
        {!selectedFile ? (
          <div className="flex flex-1 items-center justify-center rounded-2xl border border-dashed border-slate-300 bg-white text-sm text-slate-400">
            Select a log file from the sidebar.
          </div>
        ) : (
          <>
            {/* Toolbar */}
            <div className="flex flex-wrap items-center gap-2 rounded-2xl border border-slate-200 bg-white p-3 shadow-sm">
              <span className="mr-auto text-sm font-semibold text-slate-800 truncate max-w-xs" title={selectedFile}>
                {selectedFile}
              </span>

              {/* Search */}
              <input
                type="text"
                placeholder="Search lines..."
                className="rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-xs text-slate-800 outline-none transition focus:border-sky-500"
                style={{ width: '10rem' }}
                value={searchTerm}
                onChange={(e) => setSearchTerm(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleSearch();
                }}
              />
              <button
                type="button"
                className="rounded-lg bg-sky-600 px-3 py-1.5 text-xs font-semibold text-white transition hover:bg-sky-700"
                onClick={handleSearch}
              >
                Search
              </button>

              {/* Timestamp jump */}
              <input
                type="text"
                placeholder="Jump to timestamp..."
                className="rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-xs text-slate-800 outline-none transition focus:border-sky-500"
                style={{ width: '10rem' }}
                value={timestampJump}
                onChange={(e) => setTimestampJump(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleTimestampJump();
                }}
              />
              <button
                type="button"
                className="rounded-lg bg-slate-600 px-3 py-1.5 text-xs font-semibold text-white transition hover:bg-slate-700"
                onClick={handleTimestampJump}
              >
                Jump
              </button>

              {/* Tail toggle */}
              <button
                type="button"
                className={cn(
                  'rounded-lg px-3 py-1.5 text-xs font-semibold transition',
                  tailMode
                    ? 'bg-emerald-600 text-white'
                    : 'bg-slate-200 text-slate-700 hover:bg-slate-300',
                )}
                onClick={() => setTailMode((prev) => !prev)}
              >
                {tailMode ? 'Tail ON' : 'Tail'}
              </button>

              {/* Download */}
              <button
                type="button"
                className="rounded-lg bg-slate-200 px-3 py-1.5 text-xs font-semibold text-slate-700 transition hover:bg-slate-300"
                onClick={handleDownload}
              >
                Download
              </button>
            </div>

            {/* Content viewer */}
            {contentLoading && !content ? (
              <div className="flex flex-1 items-center justify-center">
                <LoadingSpinner label="Loading content" />
              </div>
            ) : contentError ? (
              <div className="rounded-2xl border border-rose-200 bg-rose-50 p-4 text-sm text-rose-700">
                {contentError}
              </div>
            ) : content ? (
              <>
                <div
                  ref={contentRef}
                  className="flex-1 overflow-auto rounded-2xl border border-slate-200 bg-slate-950 p-0 font-mono text-xs leading-6 text-slate-200"
                  style={{ maxHeight: '32rem' }}
                >
                  <table className="w-full border-collapse">
                    <tbody>
                      {displayLines.map((line, i) => {
                        const lineNum = offset + i + 1;
                        return (
                          <tr
                            key={lineNum}
                            data-line-index={i}
                            className="transition-colors hover:bg-slate-800/50"
                          >
                            <td className="select-none border-r border-slate-800 px-3 py-0 text-right text-slate-600">
                              {lineNum}
                            </td>
                            <td className="whitespace-pre-wrap break-all px-3 py-0">{line}</td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>

                {/* Pagination */}
                {totalPages > 1 && (
                  <div className="flex items-center justify-center gap-3 text-sm text-slate-600">
                    <button
                      type="button"
                      className="rounded-lg bg-white px-3 py-1 text-xs font-semibold transition hover:bg-slate-100 disabled:opacity-40"
                      disabled={currentPage <= 1}
                      onClick={() => fetchContent(selectedFile, Math.max(0, offset - LOG_PAGE_SIZE))}
                    >
                      Previous
                    </button>
                    <span>
                      Page {currentPage} of {totalPages} ({content.total} lines)
                    </span>
                    <button
                      type="button"
                      className="rounded-lg bg-white px-3 py-1 text-xs font-semibold transition hover:bg-slate-100 disabled:opacity-40"
                      disabled={currentPage >= totalPages}
                      onClick={() => fetchContent(selectedFile, offset + LOG_PAGE_SIZE)}
                    >
                      Next
                    </button>
                  </div>
                )}
              </>
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}

/* ================================================================== */
/*  Structured Events tab                                              */
/* ================================================================== */

function StructuredEventsTab() {
  const [files, setFiles] = React.useState<ApiLogFile[]>([]);
  const [loading, setLoading] = React.useState(true);
  const [selectedFile, setSelectedFile] = React.useState<string | null>(null);
  const [events, setEvents] = React.useState<StructuredEvent[]>([]);
  const [eventsLoading, setEventsLoading] = React.useState(false);
  const [filters, setFilters] = React.useState<Record<string, string>>({});

  // Load JSONL files (runs once on mount)
  const initialSelectedFile = React.useRef<string | null>(null);
  React.useEffect(() => {
    let cancelled = false;
    getLogs()
      .then((result) => {
        if (!cancelled) {
          const jsonlFiles = result.filter(
            (f) => f.path.endsWith('.jsonl') || f.path.endsWith('.json'),
          );
          setFiles(jsonlFiles);
          if (jsonlFiles.length > 0 && !initialSelectedFile.current) {
            const eventsFile = jsonlFiles.find((f) => f.path.includes('events.jsonl'));
            const target = eventsFile?.path ?? jsonlFiles[0]!.path;
            initialSelectedFile.current = target;
            setSelectedFile(target);
          }
        }
      })
      .catch(() => {
        /* ignore */
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Load file content and parse
  React.useEffect(() => {
    if (!selectedFile) return;
    let cancelled = false;
    setEventsLoading(true);
    setFilters({});
    getLogContent(selectedFile, { offset: 0, limit: 10000 })
      .then((result) => {
        if (!cancelled) {
          setEvents(parseJsonlLines(result.lines));
        }
      })
      .catch(() => {
        if (!cancelled) setEvents([]);
      })
      .finally(() => {
        if (!cancelled) setEventsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedFile]);

  // Extract filterable columns from parsed events
  const filterColumns = React.useMemo(() => {
    const cols = new Set<string>();
    for (const ev of events) {
      if (!ev.parsed) continue;
      for (const key of ['type', 'status', 'source', 'phase', 'task_id']) {
        if (typeof ev.parsed[key] === 'string') cols.add(key);
      }
    }
    return Array.from(cols).sort();
  }, [events]);

  // Unique values for each filter column
  const filterOptions = React.useMemo(() => {
    const map: Record<string, string[]> = {};
    for (const col of filterColumns) {
      const values = new Set<string>();
      for (const ev of events) {
        const val = ev.parsed?.[col];
        if (typeof val === 'string') values.add(val);
      }
      map[col] = Array.from(values).sort();
    }
    return map;
  }, [events, filterColumns]);

  // Apply filters
  const filtered = React.useMemo(() => {
    return events.filter((ev) => {
      if (!ev.parsed) return Object.keys(filters).length === 0;
      for (const [key, value] of Object.entries(filters)) {
        if (!value) continue;
        if (String(ev.parsed[key] ?? '') !== value) return false;
      }
      return true;
    });
  }, [events, filters]);

  return (
    <div className="flex flex-col gap-4">
      {/* File selector and filters */}
      <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-slate-200 bg-white p-3 shadow-sm">
        <label className="text-xs font-semibold uppercase tracking-[0.14em] text-slate-500" htmlFor="jsonl-file-select">
          File
        </label>
        <select
          id="jsonl-file-select"
          className="rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-xs text-slate-800 outline-none transition focus:border-sky-500"
          value={selectedFile ?? ''}
          onChange={(e) => setSelectedFile(e.target.value || null)}
        >
          {loading ? (
            <option value="">Loading...</option>
          ) : files.length === 0 ? (
            <option value="">No JSONL files found</option>
          ) : (
            files.map((f) => (
              <option key={f.path} value={f.path}>
                {f.path}
              </option>
            ))
          )}
        </select>

        {filterColumns.map((col) => (
          <React.Fragment key={col}>
            <label className="text-xs font-semibold uppercase tracking-[0.14em] text-slate-500" htmlFor={`filter-${col}`}>
              {col}
            </label>
            <select
              id={`filter-${col}`}
              className="rounded-lg border border-slate-300 bg-white px-2 py-1.5 text-xs text-slate-800 outline-none transition focus:border-sky-500"
              value={filters[col] ?? ''}
              onChange={(e) =>
                setFilters((prev) => ({ ...prev, [col]: e.target.value }))
              }
            >
              <option value="">All</option>
              {(filterOptions[col] ?? []).map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          </React.Fragment>
        ))}

        <span className="ml-auto text-xs text-slate-500">
          {filtered.length} of {events.length} events
        </span>
      </div>

      {/* Events table */}
      {eventsLoading ? (
        <div className="flex justify-center py-8">
          <LoadingSpinner label="Loading events" />
        </div>
      ) : filtered.length === 0 ? (
        <div className="flex items-center justify-center rounded-2xl border border-dashed border-slate-300 bg-white py-12 text-sm text-slate-400">
          {events.length === 0 ? 'No events found in this file.' : 'No events match the current filters.'}
        </div>
      ) : (
        <div className="overflow-auto rounded-2xl border border-slate-200 bg-white shadow-sm" style={{ maxHeight: '36rem' }}>
          <table className="w-full border-collapse text-xs">
            <thead className="sticky top-0 bg-slate-50">
              <tr>
                <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
                  #
                </th>
                <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
                  Timestamp
                </th>
                <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
                  Type
                </th>
                <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
                  Status
                </th>
                <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
                  Source
                </th>
                <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
                  Task
                </th>
                <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
                  Phase
                </th>
                <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
                  Details
                </th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((ev) => {
                if (!ev.parsed) {
                  return (
                    <tr key={ev.line} className="border-b border-slate-100 hover:bg-slate-50">
                      <td className="px-3 py-1.5 text-slate-400">{ev.line}</td>
                      <td colSpan={7} className="px-3 py-1.5 font-mono text-slate-500 truncate max-w-md">
                        {ev.raw}
                      </td>
                    </tr>
                  );
                }
                const p = ev.parsed;
                return (
                  <tr key={ev.line} className="border-b border-slate-100 hover:bg-slate-50">
                    <td className="px-3 py-1.5 text-slate-400">{ev.line}</td>
                    <td className="px-3 py-1.5 text-slate-600 whitespace-nowrap">
                      {typeof p.ts === 'string' ? formatTimestamp(p.ts) : '-'}
                    </td>
                    <td className="px-3 py-1.5">
                      <span className="inline-block rounded-full bg-sky-100 px-2 py-0.5 text-[10px] font-semibold uppercase text-sky-800">
                        {String(p.type ?? '-')}
                      </span>
                    </td>
                    <td className="px-3 py-1.5 text-slate-700">{String(p.status ?? '-')}</td>
                    <td className="px-3 py-1.5 text-slate-600">{String(p.source ?? '-')}</td>
                    <td className="px-3 py-1.5 text-slate-600 font-mono">
                      {String(p.task_id ?? '-')}
                    </td>
                    <td className="px-3 py-1.5 text-slate-600">{String(p.phase ?? '-')}</td>
                    <td className="px-3 py-1.5 text-slate-500 truncate max-w-xs" title={ev.raw}>
                      {typeof p.msg === 'string'
                        ? p.msg
                        : typeof p.detail === 'string'
                          ? p.detail
                          : '-'}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

/* ================================================================== */
/*  Main Logs component                                                */
/* ================================================================== */

const Logs: React.FC = () => {
  const [activeTab, setActiveTab] = React.useState<TabId>('stream');

  return (
    <section className="mx-auto flex w-full max-w-6xl flex-col gap-6 text-slate-700">
      <header className="rounded-[2rem] border border-slate-200 bg-[radial-gradient(circle_at_top_left,_rgba(56,189,248,0.16),_transparent_38%),linear-gradient(135deg,#ffffff,#f8fafc)] p-6 shadow-sm">
        <div className="flex flex-col gap-5 lg:flex-row lg:items-end lg:justify-between">
          <div className="max-w-2xl">
            <p className="text-sm font-semibold uppercase tracking-[0.2em] text-sky-700">
              Log explorer
            </p>
            <h1 className="mb-3 mt-2 text-5xl font-semibold tracking-tight text-slate-950">
              Logs
            </h1>
            <p className="max-w-xl text-base leading-7 text-slate-600">
              Browse log files by category, search within files, view live coordinator events, and
              explore structured JSONL events with filters.
            </p>
          </div>

          <div className="flex gap-2">
            <TabButton
              active={activeTab === 'stream'}
              label="Live Stream"
              onClick={() => setActiveTab('stream')}
            />
            <TabButton
              active={activeTab === 'browser'}
              label="File Browser"
              onClick={() => setActiveTab('browser')}
            />
            <TabButton
              active={activeTab === 'events'}
              label="Structured Events"
              onClick={() => setActiveTab('events')}
            />
          </div>
        </div>
      </header>

      {activeTab === 'stream' && <LiveStreamTab />}
      {activeTab === 'browser' && <FileBrowserTab />}
      {activeTab === 'events' && <StructuredEventsTab />}
    </section>
  );
};

export default Logs;
