import React from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import { useNavigate } from 'react-router-dom';
import {
  createWorktree,
  getDoctorReport,
  runApply,
  runPlan,
} from '../api/client';
import type { ApiCoordinatorAction } from '../api/models';
import { useCoordinatorStore } from '../store';

type CommandGroup = 'Navigation' | 'Coordinator' | 'Config' | 'Tools';

interface CommandItem {
  id: string;
  label: string;
  group: CommandGroup;
  keywords: string[];
  shortcut: string;
  run: () => Promise<unknown> | void;
}

const GROUP_ORDER: CommandGroup[] = ['Navigation', 'Coordinator', 'Config', 'Tools'];
const TOP_HIT_LIMIT = 50;

function normalize(value: string): string {
  return value.trim().toLowerCase();
}

function fuzzyScore(query: string, text: string): number | null {
  const q = normalize(query);
  if (!q) {
    return 1;
  }

  const hay = normalize(text);
  let score = 0;
  let lastIndex = -1;

  for (const token of q) {
    const nextIndex = hay.indexOf(token, lastIndex + 1);
    if (nextIndex === -1) {
      return null;
    }

    score += 2;
    if (nextIndex === lastIndex + 1) {
      score += 3;
    }
    if (nextIndex === 0) {
      score += 4;
    }
    lastIndex = nextIndex;
  }

  if (hay.includes(q)) {
    score += 8;
  }

  return score;
}

function createDefaultSlug(): string {
  const suffix = new Date().toISOString().replace(/[-:TZ.]/g, '').slice(0, 10);
  return `quick-${suffix}`;
}

interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const CommandPalette: React.FC<CommandPaletteProps> = ({ open, onOpenChange }) => {
  const navigate = useNavigate();
  const runCoordinatorAction = useCoordinatorStore((state) => state.runAction);
  const [query, setQuery] = React.useState('');
  const [selectedIndex, setSelectedIndex] = React.useState(0);

  const commands = React.useMemo<CommandItem[]>(() => {
    const nav = (path: string) => () => navigate(path);
    const coordinator = (action: ApiCoordinatorAction) => () => runCoordinatorAction(action);

    return [
      { id: 'nav-dashboard', label: 'Go to Dashboard', group: 'Navigation', keywords: ['home', 'overview'], shortcut: 'G D', run: nav('/dashboard') },
      { id: 'nav-welcome', label: 'Go to Welcome', group: 'Navigation', keywords: ['setup'], shortcut: 'G W', run: nav('/welcome') },
      { id: 'nav-init', label: 'Go to Init', group: 'Navigation', keywords: ['bootstrap'], shortcut: 'G I', run: nav('/init') },
      { id: 'nav-console', label: 'Go to Console', group: 'Navigation', keywords: ['ops', 'terminal'], shortcut: 'G C', run: nav('/ops/console') },
      { id: 'nav-worktrees', label: 'Go to Worktrees', group: 'Navigation', keywords: ['ops'], shortcut: 'G T', run: nav('/ops/worktrees') },
      { id: 'nav-registry', label: 'Go to Registry', group: 'Navigation', keywords: ['tasks'], shortcut: 'G R', run: nav('/ops/registry') },
      { id: 'nav-live', label: 'Go to Live', group: 'Navigation', keywords: ['events'], shortcut: 'G L', run: nav('/ops/live') },
      { id: 'nav-locks', label: 'Go to Locks', group: 'Navigation', keywords: ['ops'], shortcut: 'G K', run: nav('/ops/locks') },
      { id: 'nav-diagnostics', label: 'Go to Diagnostics', group: 'Navigation', keywords: ['doctor'], shortcut: 'G X', run: nav('/ops/diagnostics') },
      { id: 'nav-logs', label: 'Go to Logs', group: 'Navigation', keywords: ['ops'], shortcut: 'G O', run: nav('/ops/logs') },
      { id: 'nav-backups', label: 'Go to Backups', group: 'Navigation', keywords: ['ops'], shortcut: 'G B', run: nav('/ops/backups') },
      { id: 'nav-git', label: 'Go to Git Graph', group: 'Navigation', keywords: ['ops', 'commits'], shortcut: 'G G', run: nav('/ops/git') },
      { id: 'nav-tools', label: 'Go to Tools', group: 'Navigation', keywords: ['config'], shortcut: 'G U', run: nav('/config/tools') },
      { id: 'nav-standards', label: 'Go to Standards', group: 'Navigation', keywords: ['config'], shortcut: 'G S', run: nav('/config/standards') },
      { id: 'nav-skills', label: 'Go to Skills', group: 'Navigation', keywords: ['config'], shortcut: 'G H', run: nav('/config/skills') },
      { id: 'nav-settings', label: 'Go to Settings', group: 'Navigation', keywords: ['config'], shortcut: 'G E', run: nav('/config/settings') },
      { id: 'nav-help', label: 'Go to Help', group: 'Navigation', keywords: ['docs'], shortcut: 'G ?', run: nav('/help') },
      { id: 'nav-about', label: 'Go to About', group: 'Navigation', keywords: ['info'], shortcut: 'G A', run: nav('/about') },

      { id: 'coord-run', label: 'Run Coordinator', group: 'Coordinator', keywords: ['start'], shortcut: 'R', run: coordinator('run') },
      { id: 'coord-stop', label: 'Stop Coordinator', group: 'Coordinator', keywords: ['halt', 'pause'], shortcut: 'S', run: coordinator('stop') },
      { id: 'coord-resume', label: 'Resume Coordinator', group: 'Coordinator', keywords: ['continue'], shortcut: 'U', run: coordinator('resume') },

      { id: 'config-open-prd', label: 'Open PRD', group: 'Config', keywords: ['requirements'], shortcut: 'P', run: nav('/prd') },
      {
        id: 'config-create-worktree',
        label: 'Create Worktree',
        group: 'Config',
        keywords: ['branch', 'workspace'],
        shortcut: 'W',
        run: async () => {
          await createWorktree({
            slug: createDefaultSlug(),
            tool: 'codex',
            count: 1,
            base: 'main',
            skipApply: true,
            allowUserScope: true,
          });
          navigate('/ops/worktrees');
        },
      },

      {
        id: 'tools-run-plan',
        label: 'Run Plan',
        group: 'Tools',
        keywords: ['preview', 'diff'],
        shortcut: 'P L',
        run: async () => {
          await runPlan({ includeDiff: true, explain: true });
          navigate('/plan');
        },
      },
      {
        id: 'tools-run-apply',
        label: 'Run Apply',
        group: 'Tools',
        keywords: ['execute', 'deploy'],
        shortcut: 'A P',
        run: async () => {
          await runApply({ dryRun: false, yes: true, allowUserScope: true });
          navigate('/apply');
        },
      },
      {
        id: 'tools-run-doctor',
        label: 'Run Doctor',
        group: 'Tools',
        keywords: ['diagnostics', 'health'],
        shortcut: 'D R',
        run: async () => {
          await getDoctorReport();
          navigate('/ops/diagnostics');
        },
      },
    ];
  }, [navigate, runCoordinatorAction]);

  const filteredCommands = React.useMemo(() => {
    const rows = commands
      .map((command) => {
        const score = fuzzyScore(query, `${command.label} ${command.keywords.join(' ')}`);
        return score === null ? null : { command, score };
      })
      .filter((value): value is { command: CommandItem; score: number } => value !== null)
      .sort((a, b) => b.score - a.score || a.command.label.localeCompare(b.command.label))
      .slice(0, TOP_HIT_LIMIT);

    return rows.map((row) => row.command);
  }, [commands, query]);

  const groupedCommands = React.useMemo(() => {
    const groups = new Map<CommandGroup, CommandItem[]>();
    for (const item of filteredCommands) {
      const current = groups.get(item.group);
      if (current) {
        current.push(item);
      } else {
        groups.set(item.group, [item]);
      }
    }
    return groups;
  }, [filteredCommands]);

  React.useEffect(() => {
    setSelectedIndex(0);
  }, [query, open]);

  React.useEffect(() => {
    if (!open) {
      setQuery('');
    }
  }, [open]);

  const execute = React.useCallback(async (item: CommandItem) => {
    onOpenChange(false);
    try {
      await item.run();
    } catch (error) {
      // Keep command failures visible in dev tools until centralized toasts are wired.
      console.error('Command palette action failed:', error);
    }
  }, [onOpenChange]);

  const onInputKeyDown = React.useCallback((event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      if (filteredCommands.length > 0) {
        setSelectedIndex((value) => (value + 1) % filteredCommands.length);
      }
      return;
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault();
      if (filteredCommands.length > 0) {
        setSelectedIndex((value) => (value - 1 + filteredCommands.length) % filteredCommands.length);
      }
      return;
    }

    if (event.key === 'Enter') {
      event.preventDefault();
      const selected = filteredCommands[selectedIndex];
      if (selected) {
        void execute(selected);
      }
    }
  }, [execute, filteredCommands, selectedIndex]);

  const commandIndexById = React.useMemo(() => {
    const map = new Map<string, number>();
    filteredCommands.forEach((command, index) => {
      map.set(command.id, index);
    });
    return map;
  }, [filteredCommands]);

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/60 backdrop-blur-[1px]" />
        <Dialog.Content
          aria-describedby="command-palette-description"
          className="fixed left-1/2 top-[12%] z-50 w-[min(92vw,44rem)] -translate-x-1/2 overflow-hidden rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] shadow-2xl outline-none"
        >
          <Dialog.Title className="sr-only">Command palette</Dialog.Title>
          <Dialog.Description id="command-palette-description" className="sr-only">
            Search commands and run actions quickly with keyboard.
          </Dialog.Description>

          <div className="border-b border-[var(--border)] p-3">
            <input
              autoFocus
              className="h-10 w-full rounded-md border border-[var(--border)] bg-[var(--bg-card)] px-3 text-sm text-[var(--text-primary)] outline-none ring-0 placeholder:text-[var(--text-muted)] focus:border-[var(--accent)]"
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={onInputKeyDown}
              placeholder="Type a command or search routes..."
              value={query}
            />
          </div>

          <div className="max-h-[min(64vh,30rem)] overflow-y-auto p-2">
            {filteredCommands.length === 0 ? (
              <div className="px-3 py-8 text-center text-sm text-[var(--text-muted)]">
                No commands match your search.
              </div>
            ) : (
              GROUP_ORDER.map((group) => {
                const items = groupedCommands.get(group);
                if (!items || items.length === 0) {
                  return null;
                }

                return (
                  <div key={group} className="mb-2">
                    <div className="px-3 py-2 text-xs uppercase tracking-wider text-[var(--text-muted)]">
                      {group}
                    </div>
                    <ul className="space-y-1">
                      {items.map((item) => {
                        const commandIndex = commandIndexById.get(item.id) ?? -1;
                        const isSelected = commandIndex === selectedIndex;
                        return (
                          <li key={item.id}>
                            <button
                              className={`flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition-colors ${
                                isSelected
                                  ? 'bg-[var(--bg-card)] text-[var(--text-primary)]'
                                  : 'text-[var(--text-secondary)] hover:bg-[var(--bg-card)] hover:text-[var(--text-primary)]'
                              }`}
                              onClick={() => void execute(item)}
                              onMouseEnter={() => setSelectedIndex(commandIndex)}
                              type="button"
                            >
                              <span>{item.label}</span>
                              <span className="rounded border border-[var(--border)] bg-black/20 px-1.5 py-0.5 font-mono text-xs text-[var(--text-muted)]">
                                {item.shortcut}
                              </span>
                            </button>
                          </li>
                        );
                      })}
                    </ul>
                  </div>
                );
              })
            )}
          </div>

          <div className="flex items-center justify-between border-t border-[var(--border)] px-3 py-2 text-xs text-[var(--text-muted)]">
            <span>Use ↑↓ to select, Enter to run, Esc to close.</span>
            <span className="rounded border border-[var(--border)] px-1.5 py-0.5 font-mono">Ctrl+K</span>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
};

export default CommandPalette;
