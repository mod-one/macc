import React from 'react';
import { useNavigate } from 'react-router-dom';
import { useGlobalSearchStore } from '../stores/globalSearchStore';
import type { ApiConfigResponse, ApiLogFile, ApiRegistryTask, ApiWorktree } from '../api/models';
import { cn } from './styles';
import { Icons } from './NavIcons';

type SearchGroup = 'Tasks' | 'Worktrees' | 'Settings' | 'Logs';
type SearchKind = 'task' | 'worktree' | 'setting' | 'log';

interface SearchNavigationState {
  selectedTaskId?: string;
  selectedWorktreeId?: string;
  selectedLogPath?: string;
  highlightSettingKey?: string;
}

interface SearchPreviewField {
  label: string;
  value: string;
}

interface SearchResult {
  id: string;
  kind: SearchKind;
  group: SearchGroup;
  label: string;
  description: string;
  score: number;
  route: string;
  routeState: SearchNavigationState;
  preview: SearchPreviewField[];
  searchText: string;
}

const GROUP_ORDER: SearchGroup[] = ['Tasks', 'Worktrees', 'Settings', 'Logs'];

function normalize(value: string): string {
  return value.trim().toLowerCase();
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function stringifyValue(value: unknown): string {
  if (value === null || value === undefined) return '—';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  if (Array.isArray(value)) {
    return value.length === 0 ? '[]' : value.map((entry) => stringifyValue(entry)).join(', ');
  }
  if (typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>);
    if (entries.length === 0) return '{}';
    return entries
      .slice(0, 4)
      .map(([key, entry]) => `${key}: ${stringifyValue(entry)}`)
      .join(', ');
  }
  return String(value);
}

function joinSearchText(parts: Array<string | number | boolean | null | undefined>): string {
  return parts
    .map((part) => (part === null || part === undefined ? '' : String(part)))
    .filter(Boolean)
    .join(' ');
}

function scoreText(query: string, text: string): number | null {
  const q = normalize(query);
  const haystack = normalize(text);
  if (!q) return 1;
  if (!haystack.includes(q)) return null;
  if (haystack === q) return 200;

  let score = 50;
  if (haystack.startsWith(q)) {
    score += 40;
  }

  for (const token of q.split(/\s+/).filter(Boolean)) {
    if (haystack.includes(token)) {
      score += 5;
    }
  }

  score -= Math.min(haystack.indexOf(q), 30);
  return score;
}

function buildTaskResults(tasks: ApiRegistryTask[]): SearchResult[] {
  return tasks.map((task) => {
    const searchText = joinSearchText([
      task.id,
      task.title,
      task.tool,
      task.state,
      task.description,
      task.objective,
      task.result,
      task.notes,
      task.currentPhase,
      ...(task.steps ?? []),
    ]);

    return {
      id: task.id,
      kind: 'task',
      group: 'Tasks',
      label: task.title?.trim() || task.id,
      description: task.state,
      score: 0,
      route: '/ops/registry',
      routeState: { selectedTaskId: task.id },
      preview: [
        { label: 'Task ID', value: task.id },
        { label: 'State', value: task.state },
        { label: 'Tool', value: task.tool || '—' },
        { label: 'Phase', value: task.currentPhase || '—' },
        { label: 'Objective', value: task.objective || task.description || '—' },
      ],
      searchText,
    };
  });
}

function buildWorktreeResults(worktrees: ApiWorktree[]): SearchResult[] {
  return worktrees.map((worktree) => {
    const searchText = joinSearchText([
      worktree.id,
      worktree.slug,
      worktree.branch,
      worktree.tool,
      worktree.status,
      worktree.path,
      worktree.baseBranch,
      worktree.scope,
      worktree.feature,
      worktree.sessionLabel,
    ]);

    return {
      id: worktree.id,
      kind: 'worktree',
      group: 'Worktrees',
      label: worktree.slug?.trim() || worktree.id,
      description: worktree.branch || worktree.path,
      score: 0,
      route: '/ops/worktrees',
      routeState: { selectedWorktreeId: worktree.id },
      preview: [
        { label: 'Worktree ID', value: worktree.id },
        { label: 'Branch', value: worktree.branch || '—' },
        { label: 'Path', value: worktree.path },
        { label: 'Status', value: worktree.status || '—' },
        { label: 'Scope', value: worktree.scope || '—' },
      ],
      searchText,
    };
  });
}

function pushSettingResult(
  results: SearchResult[],
  key: string,
  value: unknown,
  routeState: SearchNavigationState,
): void {
  const textValue = stringifyValue(value);
  const searchText = joinSearchText([key, textValue]);
  results.push({
    id: key,
    kind: 'setting',
    group: 'Settings',
    label: key,
    description: textValue,
    score: 0,
    route: '/config/settings',
    routeState,
    preview: [
      { label: 'Setting', value: key },
      { label: 'Value', value: textValue },
    ],
    searchText,
  });
}

function buildSettingResults(config: ApiConfigResponse | null): SearchResult[] {
  if (!config) return [];

  const results: SearchResult[] = [];

  for (const [key, value] of Object.entries(config)) {
    if (value === null || value === undefined) {
      continue;
    }

    if (Array.isArray(value)) {
      if (value.length === 0) {
        pushSettingResult(results, key, '[]', { highlightSettingKey: key });
        continue;
      }

      value.forEach((entry, index) => {
        pushSettingResult(results, `${key}[${index}]`, entry, { highlightSettingKey: key });
      });
      continue;
    }

    if (typeof value === 'object') {
      const entries = Object.entries(value as Record<string, unknown>);
      if (entries.length === 0) {
        pushSettingResult(results, key, '{}', { highlightSettingKey: key });
        continue;
      }

      for (const [nestedKey, nestedValue] of entries) {
        pushSettingResult(results, `${key}.${nestedKey}`, nestedValue, { highlightSettingKey: key });
      }
      continue;
    }

    pushSettingResult(results, key, value, { highlightSettingKey: key });
  }

  return results;
}

function buildLogResults(logs: ApiLogFile[]): SearchResult[] {
  return logs.map((log) => {
    const searchText = joinSearchText([log.path, log.size, log.modified]);
    return {
      id: log.path,
      kind: 'log',
      group: 'Logs',
      label: log.path,
      description: `${formatBytes(log.size)}${log.modified ? ` • ${log.modified}` : ''}`,
      score: 0,
      route: '/ops/logs',
      routeState: { selectedLogPath: log.path },
      preview: [
        { label: 'Path', value: log.path },
        { label: 'Size', value: formatBytes(log.size) },
        { label: 'Modified', value: log.modified || '—' },
      ],
      searchText,
    };
  });
}

const GlobalSearch: React.FC = () => {
  const navigate = useNavigate();
  const rootRef = React.useRef<HTMLDivElement>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);
  const didRequestLoadRef = React.useRef(false);
  const selectedIndexRef = React.useRef(0);
  const [query, setQuery] = React.useState('');
  const [debouncedQuery, setDebouncedQuery] = React.useState('');
  const [isOpen, setIsOpen] = React.useState(false);
  const [selectedIndex, setSelectedIndex] = React.useState(0);

  const { tasks, worktrees, config, logs, isLoading, error, loadGlobalSearchData } =
    useGlobalSearchStore((state) => state);

  React.useEffect(() => {
    if (didRequestLoadRef.current) {
      return;
    }

    didRequestLoadRef.current = true;
    void loadGlobalSearchData();
  }, [loadGlobalSearchData]);

  React.useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebouncedQuery(query);
    }, 300);

    return () => window.clearTimeout(timer);
  }, [query]);

  const allResults = React.useMemo(() => {
    const queryText = normalize(debouncedQuery);
    if (!queryText) {
      return [];
    }

    const results = [
      ...buildTaskResults(tasks),
      ...buildWorktreeResults(worktrees),
      ...buildSettingResults(config),
      ...buildLogResults(logs),
    ]
      .map((result) => ({
        ...result,
        score: scoreText(queryText, result.searchText) ?? 0,
      }))
      .filter((result) => result.score > 0)
      .sort((left, right) => {
        const groupDelta = GROUP_ORDER.indexOf(left.group) - GROUP_ORDER.indexOf(right.group);
        if (groupDelta !== 0) return groupDelta;
        return right.score - left.score || left.label.localeCompare(right.label);
      });

    return results.map((result, index) => ({ ...result, index }));
  }, [config, debouncedQuery, logs, tasks, worktrees]);

  const groupedResults = React.useMemo(() => {
    const groups = new Map<SearchGroup, Array<SearchResult & { index: number }>>();
    for (const group of GROUP_ORDER) {
      groups.set(group, []);
    }

    for (const result of allResults) {
      groups.get(result.group)?.push(result);
    }

    return groups;
  }, [allResults]);

  React.useEffect(() => {
    selectedIndexRef.current = selectedIndex;
  }, [selectedIndex]);

  React.useEffect(() => {
    if (!isOpen) {
      setSelectedIndex(0);
      selectedIndexRef.current = 0;
      return;
    }

    if (allResults.length === 0) {
      setSelectedIndex(0);
      selectedIndexRef.current = 0;
      return;
    }

    setSelectedIndex((current) => {
      const next = Math.min(current, allResults.length - 1);
      selectedIndexRef.current = next;
      return next;
    });
  }, [allResults, isOpen]);

  React.useEffect(() => {
    const handleGlobalShortcut = (event: KeyboardEvent) => {
      const key = event.key === '?' ? '/' : event.key;
      if ((event.ctrlKey || event.metaKey) && key === '/') {
        event.preventDefault();
        setIsOpen(true);
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    };

    const handlePointerDown = (event: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };

    window.addEventListener('keydown', handleGlobalShortcut);
    window.addEventListener('pointerdown', handlePointerDown);

    return () => {
      window.removeEventListener('keydown', handleGlobalShortcut);
      window.removeEventListener('pointerdown', handlePointerDown);
    };
  }, []);

  const navigateToResult = React.useCallback(
    (result: SearchResult) => {
      navigate(result.route, { state: result.routeState });
      setIsOpen(false);
      setQuery('');
      setDebouncedQuery('');
    },
    [navigate],
  );

  const handleInputKeyDown = React.useCallback(
    (event: React.KeyboardEvent<HTMLInputElement>) => {
      if (!isOpen && event.key !== 'Enter') {
        setIsOpen(true);
      }

      if (event.key === 'ArrowDown') {
        event.preventDefault();
        if (allResults.length > 0) {
          setSelectedIndex((value) => {
            const next = (value + 1) % allResults.length;
            selectedIndexRef.current = next;
            return next;
          });
        }
        return;
      }

      if (event.key === 'ArrowUp') {
        event.preventDefault();
        if (allResults.length > 0) {
          setSelectedIndex((value) => {
            const next = (value - 1 + allResults.length) % allResults.length;
            selectedIndexRef.current = next;
            return next;
          });
        }
        return;
      }

      if (event.key === 'Enter') {
        event.preventDefault();
        const selected = allResults[selectedIndexRef.current];
        if (selected) {
          navigateToResult(selected);
        }
        return;
      }

      if (event.key === 'Escape') {
        event.preventDefault();
        setIsOpen(false);
      }
    },
    [allResults, isOpen, navigateToResult],
  );

  const selectedResult = allResults[selectedIndex] ?? null;
  const hasQuery = normalize(query).length > 0;

  return (
    <div ref={rootRef} className="relative flex w-full justify-center">
      <div className="relative w-full max-w-[40rem]">
        <label htmlFor="global-search-input" className="sr-only">
          Search tasks, worktrees, settings, and logs
        </label>
        <div className="relative">
          <span className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-[var(--text-muted)]">
            <Icons.Search />
          </span>
          <input
            ref={inputRef}
            id="global-search-input"
            className="h-9 w-full rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] pl-9 pr-20 text-sm text-[var(--text-primary)] outline-none transition-colors placeholder:text-[var(--text-muted)] focus:border-[var(--accent)]"
            placeholder="Search tasks, worktrees, settings, and logs"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setIsOpen(true);
            }}
            onClick={() => setIsOpen(true)}
            onFocus={() => setIsOpen(true)}
            onKeyDown={handleInputKeyDown}
            aria-expanded={isOpen}
            aria-controls="global-search-results"
            aria-autocomplete="list"
            role="combobox"
            type="search"
          />
          <kbd className="absolute right-3 top-1/2 -translate-y-1/2 rounded border border-[var(--border)] bg-[var(--bg-card)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--text-muted)]">
            Ctrl+/
          </kbd>
        </div>
      </div>

      {isOpen && (
        <div className="fixed left-1/2 top-[3.4rem] z-50 w-[min(92vw,76rem)] -translate-x-1/2 overflow-hidden rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] shadow-2xl">
          <div className="grid max-h-[min(34rem,calc(100vh-5rem))] grid-cols-1 divide-y divide-[var(--border)] lg:grid-cols-[minmax(0,1.1fr)_minmax(18rem,0.9fr)] lg:divide-y-0 lg:divide-x">
            <div className="min-h-0">
              <div className="border-b border-[var(--border)] px-4 py-3">
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <h2 className="text-sm font-semibold text-[var(--text-primary)]">Global search</h2>
                    <p className="text-xs text-[var(--text-muted)]">
                      Search tasks, worktrees, settings keys, and log file metadata.
                    </p>
                  </div>
                  {isLoading && <span className="text-xs text-[var(--text-muted)]">Loading index...</span>}
                </div>
                {error && <p className="mt-2 text-xs text-amber-400">{error}</p>}
              </div>

              <div
                id="global-search-results"
                role="listbox"
                className="max-h-[24rem] overflow-y-auto px-2 py-2"
              >
                {!hasQuery ? (
                  <div className="rounded-xl border border-dashed border-[var(--border)] px-4 py-6 text-sm text-[var(--text-muted)]">
                    Type to search across tasks, worktrees, settings, and logs.
                  </div>
                ) : allResults.length === 0 ? (
                  <div className="rounded-xl border border-dashed border-[var(--border)] px-4 py-6 text-sm text-[var(--text-muted)]">
                    No matches found for{' '}
                    <span className="font-mono text-[var(--text-primary)]">{query}</span>.
                  </div>
                ) : (
                  GROUP_ORDER.map((group) => {
                    const entries = groupedResults.get(group) ?? [];
                    if (entries.length === 0) {
                      return null;
                    }

                    return (
                      <section key={group} className="mb-2 last:mb-0">
                        <div className="px-3 py-2 text-[10px] font-semibold uppercase tracking-[0.2em] text-[var(--text-muted)]">
                          {group}
                        </div>
                        <div className="space-y-1">
                          {entries.map((result) => {
                            const isSelected = result.index === selectedIndex;
                            return (
                              <button
                                key={`${result.kind}:${result.id}`}
                                type="button"
                                className={cn(
                                  'flex w-full items-start gap-3 rounded-xl px-3 py-3 text-left transition-colors',
                                  isSelected
                                    ? 'bg-[var(--accent)]/10 ring-1 ring-inset ring-[var(--accent)]/40'
                                    : 'hover:bg-[var(--bg-card)]',
                                )}
                                onClick={() => navigateToResult(result)}
                                onMouseEnter={() => {
                                  selectedIndexRef.current = result.index;
                                  setSelectedIndex(result.index);
                                }}
                              >
                                <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-[var(--border)] bg-[var(--bg-card)] text-[10px] font-bold uppercase tracking-wider text-[var(--text-secondary)]">
                                  {result.kind.slice(0, 2)}
                                </div>
                                <div className="min-w-0 flex-1">
                                  <div className="truncate text-sm font-medium text-[var(--text-primary)]">
                                    {result.label}
                                  </div>
                                  <div className="truncate text-xs text-[var(--text-muted)]">
                                    {result.description}
                                  </div>
                                </div>
                                <div className="shrink-0 text-[10px] uppercase tracking-[0.16em] text-[var(--text-muted)]">
                                  {result.group}
                                </div>
                              </button>
                            );
                          })}
                        </div>
                      </section>
                    );
                  })
                )}
              </div>
            </div>

            <aside className="min-h-0 bg-[var(--bg-primary)] px-4 py-4">
              {selectedResult ? (
                <div className="flex h-full flex-col gap-4">
                  <div>
                    <p className="text-[10px] font-semibold uppercase tracking-[0.2em] text-[var(--text-muted)]">
                      Preview
                    </p>
                    <h3 className="mt-1 truncate text-base font-semibold text-[var(--text-primary)]">
                      {selectedResult.label}
                    </h3>
                    <p className="text-sm text-[var(--text-muted)]">{selectedResult.group}</p>
                  </div>

                  <dl className="grid gap-3 text-sm">
                    {selectedResult.preview.map((field) => (
                      <div
                        key={`${selectedResult.id}-${field.label}`}
                        className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-2"
                      >
                        <dt className="text-[10px] font-semibold uppercase tracking-[0.16em] text-[var(--text-muted)]">
                          {field.label}
                        </dt>
                        <dd className="mt-1 break-words text-[var(--text-primary)]">{field.value}</dd>
                      </div>
                    ))}
                  </dl>

                  <div className="mt-auto rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-2 text-xs text-[var(--text-muted)]">
                    Press{' '}
                    <kbd className="rounded border border-[var(--border)] bg-[var(--bg-card)] px-1.5 py-0.5 font-mono">
                      Enter
                    </kbd>{' '}
                    to open the highlighted result.
                  </div>
                </div>
              ) : (
                <div className="flex h-full items-center justify-center rounded-xl border border-dashed border-[var(--border)] px-4 py-8 text-center text-sm text-[var(--text-muted)]">
                  Search results will appear here.
                </div>
              )}
            </aside>
          </div>
        </div>
      )}
    </div>
  );
};

export default GlobalSearch;
