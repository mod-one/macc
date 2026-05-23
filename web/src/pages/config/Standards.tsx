import React from 'react';
import { getConfig, getStandardsPreview, updateConfig, ApiClientError } from '../../api/client';
import type { ApiConfigResponse, ApiStandardsPreviewCard } from '../../api/models';
import { Button, ErrorBanner, LoadingSpinner, Toast } from '../../components';
import { cn } from '../../components/styles';

type StandardsMap = Record<string, string>;
type PresetId = 'minimal' | 'strict' | 'none';
type ToastVariant = 'success' | 'error' | 'warning';

interface ToastState {
  open: boolean;
  title: string;
  description?: string;
  variant: ToastVariant;
}

interface StandardsPreset {
  id: PresetId;
  label: string;
  description: string;
  values: StandardsMap;
}

type PreviewCard = ApiStandardsPreviewCard;

const PRESETS: StandardsPreset[] = [
  {
    id: 'minimal',
    label: 'Minimal',
    description: 'Language + package manager defaults.',
    values: {
      language: 'English',
      package_manager: 'pnpm',
    },
  },
  {
    id: 'strict',
    label: 'Strict',
    description: 'Minimal + stricter TypeScript and import conventions.',
    values: {
      language: 'English',
      package_manager: 'pnpm',
      typescript: 'strict',
      imports: 'absolute:@/',
    },
  },
  {
    id: 'none',
    label: 'None',
    description: 'No preset values; define standards manually.',
    values: {},
  },
];

const PRESET_BY_ID: Record<PresetId, StandardsPreset> = {
  minimal: PRESETS[0],
  strict: PRESETS[1],
  none: PRESETS[2],
};

const EMPTY_PREVIEW_CARDS: PreviewCard[] = [
  {
    id: 'codex',
    title: 'Codex - AGENTS.md (rendered)',
    content: 'Loading preview...',
  },
  {
    id: 'claude',
    title: 'Claude - CLAUDE.md (rendered)',
    content: 'Loading preview...',
  },
  {
    id: 'gemini',
    title: 'Gemini - GEMINI.md (rendered)',
    content: 'Loading preview...',
  },
  {
    id: 'vibe',
    title: 'Vibe - AGENTS.md (rendered)',
    content: 'Loading preview...',
  },
];

function formatError(error: unknown): string {
  if (error instanceof ApiClientError) {
    return `${error.envelope.error.message} (${error.envelope.error.code})`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return 'Unexpected standards configuration error.';
}

function normalizeInlineStandards(input: StandardsMap): StandardsMap {
  const next: StandardsMap = {};
  for (const [rawKey, rawValue] of Object.entries(input)) {
    const key = rawKey.trim();
    const value = rawValue.trim();
    if (key.length === 0 || value.length === 0) {
      continue;
    }
    next[key] = value;
  }
  return next;
}

function mergePresetAndOverrides(presetId: PresetId, overrides: StandardsMap): StandardsMap {
  return normalizeInlineStandards({
    ...PRESET_BY_ID[presetId].values,
    ...overrides,
  });
}

function listKeys(map: StandardsMap): string[] {
  return Object.keys(map).sort((left, right) => left.localeCompare(right));
}

function mapEquals(left: StandardsMap, right: StandardsMap): boolean {
  const leftKeys = listKeys(left);
  const rightKeys = listKeys(right);
  if (leftKeys.length !== rightKeys.length) {
    return false;
  }
  for (const key of leftKeys) {
    if (left[key] !== right[key]) {
      return false;
    }
  }
  return true;
}

function computeOverrides(effective: StandardsMap, presetId: PresetId): StandardsMap {
  const preset = PRESET_BY_ID[presetId].values;
  const keys = new Set([...Object.keys(effective), ...Object.keys(preset)]);
  const overrides: StandardsMap = {};
  for (const key of keys) {
    const effectiveValue = effective[key];
    const presetValue = preset[key];
    if (effectiveValue !== undefined && effectiveValue !== presetValue) {
      overrides[key] = effectiveValue;
    }
  }
  return normalizeInlineStandards(overrides);
}

function pickClosestPreset(standardsInline: StandardsMap): PresetId {
  let selected: PresetId = 'minimal';
  let bestDistance = Number.POSITIVE_INFINITY;

  for (const preset of PRESETS) {
    const merged = mergePresetAndOverrides(preset.id, computeOverrides(standardsInline, preset.id));
    const keys = new Set([...Object.keys(merged), ...Object.keys(standardsInline)]);
    let distance = 0;
    for (const key of keys) {
      if (merged[key] !== standardsInline[key]) {
        distance += 1;
      }
    }
    if (distance < bestDistance) {
      bestDistance = distance;
      selected = preset.id;
    }
  }

  return selected;
}

function buildLintWarnings(
  standardsInline: StandardsMap,
  standardsPath: string | null,
  presetId: PresetId,
): string[] {
  const warnings: string[] = [];

  const language = standardsInline.language;
  if (language && language.toLowerCase() !== 'english') {
    warnings.push(`language should be "English" for consistency (current: ${language}).`);
  }
  if (!language && !standardsPath) {
    warnings.push('language is unset and no external standards path is configured.');
  }

  const packageManager = standardsInline.package_manager;
  if (packageManager && !['pnpm', 'npm', 'yarn', 'bun'].includes(packageManager)) {
    warnings.push(`package_manager is uncommon (${packageManager}); expected one of pnpm|npm|yarn|bun.`);
  }

  const tsMode = standardsInline.typescript;
  if (tsMode && !['strict', 'recommended', 'off'].includes(tsMode)) {
    warnings.push(`typescript mode "${tsMode}" is not recognized.`);
  }

  if (standardsInline.imports?.startsWith('absolute:') && standardsInline.typescript !== 'strict') {
    warnings.push('imports uses absolute mapping but typescript is not set to strict.');
  }

  if (presetId === 'strict' && standardsInline.typescript !== 'strict') {
    warnings.push('strict preset selected, but override no longer keeps typescript=strict.');
  }

  for (const key of Object.keys(standardsInline)) {
    if (/\s/.test(key)) {
      warnings.push(`standard key "${key}" contains whitespace; use snake_case style keys.`);
    }
  }

  return warnings;
}

function classifyDiffEntry(
  key: string,
  presetValue: string | undefined,
  effectiveValue: string | undefined,
): {
  key: string;
  presetValue: string | null;
  effectiveValue: string | null;
  kind: 'added' | 'changed' | 'removed' | 'unchanged';
} {
  if (presetValue === undefined && effectiveValue !== undefined) {
    return { key, presetValue: null, effectiveValue, kind: 'added' };
  }
  if (presetValue !== undefined && effectiveValue === undefined) {
    return { key, presetValue, effectiveValue: null, kind: 'removed' };
  }
  if (presetValue !== effectiveValue) {
    return {
      key,
      presetValue: presetValue ?? null,
      effectiveValue: effectiveValue ?? null,
      kind: 'changed',
    };
  }
  return {
    key,
    presetValue: presetValue ?? null,
    effectiveValue: effectiveValue ?? null,
    kind: 'unchanged',
  };
}

const Standards: React.FC = () => {
  const [isLoading, setIsLoading] = React.useState(true);
  const [isSaving, setIsSaving] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const [standardsPath, setStandardsPath] = React.useState('');
  const [selectedPreset, setSelectedPreset] = React.useState<PresetId>('minimal');
  const [overrides, setOverrides] = React.useState<StandardsMap>({});
  const [newOverrideKey, setNewOverrideKey] = React.useState('');
  const [newOverrideValue, setNewOverrideValue] = React.useState('');
  const [isPreviewLoading, setIsPreviewLoading] = React.useState(false);
  const [previewError, setPreviewError] = React.useState<string | null>(null);
  const [previewCards, setPreviewCards] = React.useState<PreviewCard[]>(EMPTY_PREVIEW_CARDS);
  const [toast, setToast] = React.useState<ToastState>({
    open: false,
    title: '',
    variant: 'success',
  });

  const baselineRef = React.useRef<{ path: string | null; inline: StandardsMap }>({
    path: null,
    inline: {},
  });

  const hydrateFromConfig = React.useCallback((config: ApiConfigResponse) => {
    const nextInline = normalizeInlineStandards(config.standardsInline);
    const presetId = pickClosestPreset(nextInline);

    baselineRef.current = {
      path: config.standardsPath,
      inline: nextInline,
    };

    setStandardsPath(config.standardsPath ?? '');
    setSelectedPreset(presetId);
    setOverrides(computeOverrides(nextInline, presetId));
    setError(null);
  }, []);

  const loadConfig = React.useCallback(async () => {
    setIsLoading(true);
    try {
      const config = await getConfig();
      hydrateFromConfig(config);
    } catch (loadError) {
      setError(formatError(loadError));
    } finally {
      setIsLoading(false);
    }
  }, [hydrateFromConfig]);

  React.useEffect(() => {
    void loadConfig();
  }, [loadConfig]);

  const effectiveInline = React.useMemo(
    () => mergePresetAndOverrides(selectedPreset, overrides),
    [selectedPreset, overrides],
  );

  const currentPath = standardsPath.trim().length > 0 ? standardsPath.trim() : null;
  const isDirty =
    currentPath !== baselineRef.current.path ||
    !mapEquals(effectiveInline, baselineRef.current.inline);

  const lintWarnings = React.useMemo(
    () => buildLintWarnings(effectiveInline, currentPath, selectedPreset),
    [currentPath, effectiveInline, selectedPreset],
  );

  const diffEntries = React.useMemo(() => {
    const presetValues = PRESET_BY_ID[selectedPreset].values;
    const keys = new Set([...Object.keys(presetValues), ...Object.keys(effectiveInline)]);
    return Array.from(keys)
      .sort((left, right) => left.localeCompare(right))
      .map((key) => classifyDiffEntry(key, presetValues[key], effectiveInline[key]));
  }, [effectiveInline, selectedPreset]);

  React.useEffect(() => {
    let active = true;
    const controller = new AbortController();
    const timer = window.setTimeout(() => {
      setIsPreviewLoading(true);
      void getStandardsPreview(
        {
          standardsPath: currentPath,
          standardsInline: effectiveInline,
        },
        { signal: controller.signal },
      )
        .then((response) => {
          if (!active) {
            return;
          }
          setPreviewCards(response.cards);
          setPreviewError(null);
        })
        .catch((previewLoadError) => {
          if (!active) {
            return;
          }
          const message = formatError(previewLoadError);
          setPreviewError(message);
          setPreviewCards(
            EMPTY_PREVIEW_CARDS.map((card) => ({
              ...card,
              content: 'Preview unavailable.',
            })),
          );
        })
        .finally(() => {
          if (active) {
            setIsPreviewLoading(false);
          }
        });
    }, 120);

    return () => {
      active = false;
      controller.abort();
      window.clearTimeout(timer);
    };
  }, [currentPath, effectiveInline]);

  const applyPreset = React.useCallback(
    (nextPreset: PresetId) => {
      if (nextPreset === selectedPreset) {
        return;
      }

      const currentEffective = mergePresetAndOverrides(selectedPreset, overrides);
      const nextOverrides = computeOverrides(currentEffective, nextPreset);
      setSelectedPreset(nextPreset);
      setOverrides(nextOverrides);
    },
    [overrides, selectedPreset],
  );

  const updateOverrideValue = React.useCallback(
    (key: string, nextValue: string) => {
      const normalizedValue = nextValue.trim();
      const presetValue = PRESET_BY_ID[selectedPreset].values[key];

      setOverrides((current) => {
        const next = { ...current };
        if (normalizedValue.length === 0 || normalizedValue === presetValue) {
          delete next[key];
        } else {
          next[key] = normalizedValue;
        }
        return next;
      });
    },
    [selectedPreset],
  );

  const removeOverride = React.useCallback((key: string) => {
    setOverrides((current) => {
      const next = { ...current };
      delete next[key];
      return next;
    });
  }, []);

  const addOverride = React.useCallback(() => {
    const key = newOverrideKey.trim();
    const value = newOverrideValue.trim();

    if (key.length === 0 || value.length === 0) {
      setToast({
        open: true,
        title: 'Invalid override',
        description: 'Both key and value are required.',
        variant: 'warning',
      });
      return;
    }

    updateOverrideValue(key, value);
    setNewOverrideKey('');
    setNewOverrideValue('');
  }, [newOverrideKey, newOverrideValue, updateOverrideValue]);

  const handleSave = React.useCallback(async () => {
    setIsSaving(true);
    setError(null);

    try {
      const updated = await updateConfig({
        standardsPath: currentPath,
        standardsInline: effectiveInline,
      });
      hydrateFromConfig(updated);
      setToast({
        open: true,
        title: 'Standards saved',
        description: 'Configuration updated successfully.',
        variant: 'success',
      });
    } catch (saveError) {
      const message = formatError(saveError);
      setError(message);
      setToast({
        open: true,
        title: 'Failed to save',
        description: message,
        variant: 'error',
      });
    } finally {
      setIsSaving(false);
    }
  }, [currentPath, effectiveInline, hydrateFromConfig]);

  const handleReset = React.useCallback(() => {
    const baselineInline = baselineRef.current.inline;
    const presetId = pickClosestPreset(baselineInline);
    setStandardsPath(baselineRef.current.path ?? '');
    setSelectedPreset(presetId);
    setOverrides(computeOverrides(baselineInline, presetId));
  }, []);

  if (isLoading) {
    return (
      <section className="flex min-h-[16rem] items-center justify-center">
        <LoadingSpinner label="Loading standards..." size="lg" className="text-sm" />
      </section>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      <header className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-6">
        <h1 className="text-3xl font-semibold text-[var(--text-primary)]">Standards</h1>
        <p className="mt-2 text-sm text-[var(--text-secondary)]">
          Select a preset, customize overrides, preview rendered tool output, and save the standards
          section.
        </p>
      </header>

      {error && <ErrorBanner title="Standards configuration error" message={error} />}

      <section className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-6">
        <h2 className="text-lg font-semibold text-[var(--text-primary)]">Preset and source</h2>
        <div className="mt-4 grid gap-4 md:grid-cols-2">
          <label className="flex flex-col gap-1.5">
            <span className="text-xs font-medium text-[var(--text-secondary)]">Preset</span>
            <select
              aria-label="Standards preset"
              className="rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-2 text-sm text-[var(--text-primary)] outline-none focus:border-[var(--accent)]"
              value={selectedPreset}
              onChange={(event) => applyPreset(event.target.value as PresetId)}
            >
              {PRESETS.map((preset) => (
                <option key={preset.id} value={preset.id}>
                  {preset.label}
                </option>
              ))}
            </select>
            <span className="text-xs text-[var(--text-muted)]">
              {PRESET_BY_ID[selectedPreset].description}
            </span>
          </label>

          <label className="flex flex-col gap-1.5">
            <span className="text-xs font-medium text-[var(--text-secondary)]">
              External standards path (optional)
            </span>
            <input
              type="text"
              value={standardsPath}
              onChange={(event) => setStandardsPath(event.target.value)}
              placeholder="docs/standards.md"
              className="rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-2 text-sm text-[var(--text-primary)] outline-none focus:border-[var(--accent)]"
            />
          </label>
        </div>
      </section>

      <section className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-6">
        <h2 className="text-lg font-semibold text-[var(--text-primary)]">Override editor</h2>
        <p className="mt-1 text-xs text-[var(--text-muted)]">
          Only values that differ from the selected preset are stored as overrides.
        </p>

        <div className="mt-4 grid gap-3 md:grid-cols-[1fr_1fr_auto]">
          <input
            type="text"
            value={newOverrideKey}
            onChange={(event) => setNewOverrideKey(event.target.value)}
            placeholder="convention key"
            className="rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-2 text-sm text-[var(--text-primary)] outline-none focus:border-[var(--accent)]"
          />
          <input
            type="text"
            value={newOverrideValue}
            onChange={(event) => setNewOverrideValue(event.target.value)}
            placeholder="value"
            className="rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-2 text-sm text-[var(--text-primary)] outline-none focus:border-[var(--accent)]"
          />
          <Button type="button" onClick={addOverride} className="h-fit">
            Add override
          </Button>
        </div>

        <div className="mt-4 overflow-auto rounded-lg border border-[var(--border)]">
          <table className="min-w-full divide-y divide-[var(--border)] text-sm">
            <thead className="bg-[var(--bg-secondary)]">
              <tr>
                <th className="px-3 py-2 text-left font-medium text-[var(--text-secondary)]">Key</th>
                <th className="px-3 py-2 text-left font-medium text-[var(--text-secondary)]">Value</th>
                <th className="px-3 py-2 text-right font-medium text-[var(--text-secondary)]">Action</th>
              </tr>
            </thead>
            <tbody>
              {listKeys(overrides).length === 0 && (
                <tr>
                  <td colSpan={3} className="px-3 py-4 text-center text-xs text-[var(--text-muted)]">
                    No overrides for this preset.
                  </td>
                </tr>
              )}
              {listKeys(overrides).map((key) => (
                <tr key={key} className="border-t border-[var(--border)]">
                  <td className="px-3 py-2 font-mono text-xs text-[var(--text-primary)]">{key}</td>
                  <td className="px-3 py-2">
                    <input
                      type="text"
                      value={overrides[key]}
                      onChange={(event) => updateOverrideValue(key, event.target.value)}
                      className="w-full rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-2 py-1.5 text-sm text-[var(--text-primary)] outline-none focus:border-[var(--accent)]"
                    />
                  </td>
                  <td className="px-3 py-2 text-right">
                    <button
                      type="button"
                      className="rounded-md border border-[var(--border)] bg-[var(--bg-secondary)] px-2 py-1 text-xs text-[var(--text-primary)] transition-colors hover:bg-black/10"
                      onClick={() => removeOverride(key)}
                    >
                      Remove
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <section className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-6">
        <h2 className="text-lg font-semibold text-[var(--text-primary)]">Diff from preset</h2>
        <div className="mt-4 overflow-auto rounded-lg border border-[var(--border)]">
          <table className="min-w-full divide-y divide-[var(--border)] text-sm">
            <thead className="bg-[var(--bg-secondary)]">
              <tr>
                <th className="px-3 py-2 text-left font-medium text-[var(--text-secondary)]">Key</th>
                <th className="px-3 py-2 text-left font-medium text-[var(--text-secondary)]">Preset</th>
                <th className="px-3 py-2 text-left font-medium text-[var(--text-secondary)]">Current</th>
                <th className="px-3 py-2 text-left font-medium text-[var(--text-secondary)]">Status</th>
              </tr>
            </thead>
            <tbody>
              {diffEntries.length === 0 && (
                <tr>
                  <td colSpan={4} className="px-3 py-4 text-center text-xs text-[var(--text-muted)]">
                    No standards to compare.
                  </td>
                </tr>
              )}
              {diffEntries.map((entry) => (
                <tr key={entry.key} className="border-t border-[var(--border)]">
                  <td className="px-3 py-2 font-mono text-xs text-[var(--text-primary)]">
                    {entry.key}
                  </td>
                  <td className="px-3 py-2 text-xs text-[var(--text-secondary)]">
                    {entry.presetValue ?? '—'}
                  </td>
                  <td className="px-3 py-2 text-xs text-[var(--text-primary)]">
                    {entry.effectiveValue ?? '—'}
                  </td>
                  <td className="px-3 py-2 text-xs">
                    <span
                      className={cn(
                        'rounded-full px-2 py-0.5 font-medium',
                        entry.kind === 'unchanged' && 'bg-emerald-500/15 text-emerald-300',
                        entry.kind === 'added' && 'bg-sky-500/15 text-sky-300',
                        entry.kind === 'changed' && 'bg-amber-500/15 text-amber-300',
                        entry.kind === 'removed' && 'bg-rose-500/15 text-rose-300',
                      )}
                    >
                      {entry.kind}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <section className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-6">
        <h2 className="text-lg font-semibold text-[var(--text-primary)]">Lint warnings</h2>
        <ul className="mt-3 space-y-2">
          {lintWarnings.length === 0 && (
            <li className="rounded-md border border-emerald-500/20 bg-emerald-500/10 px-3 py-2 text-xs text-emerald-300">
              No lint warnings detected.
            </li>
          )}
          {lintWarnings.map((warning) => (
            <li
              key={warning}
              className="rounded-md border border-amber-500/20 bg-amber-500/10 px-3 py-2 text-xs text-amber-200"
            >
              {warning}
            </li>
          ))}
        </ul>
      </section>

      <section className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-6">
        <h2 className="text-lg font-semibold text-[var(--text-primary)]">Rendered output preview</h2>
        {previewError && (
          <p className="mt-2 text-xs text-amber-200">
            Preview refresh failed: {previewError}
          </p>
        )}
        {isPreviewLoading && (
          <p className="mt-2 text-xs text-[var(--text-muted)]">Refreshing preview...</p>
        )}
        <div className="mt-4 grid gap-4 xl:grid-cols-3">
          {previewCards.map((card) => (
            <article key={card.id} className="rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] p-3">
              <h3 className="text-xs font-semibold text-[var(--text-secondary)]">{card.title}</h3>
              <pre className="mt-2 max-h-72 overflow-auto rounded-md bg-black/20 p-3 text-xs text-[var(--text-primary)]">
                {card.content}
              </pre>
            </article>
          ))}
        </div>
      </section>

      <footer className="sticky bottom-0 z-10 flex flex-wrap items-center justify-between gap-3 rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)]/95 p-4 backdrop-blur">
        <span className="text-xs text-[var(--text-secondary)]">
          {isDirty ? 'You have unsaved standards changes.' : 'No pending changes.'}
        </span>
        <div className="flex items-center gap-2">
          <Button type="button" onClick={handleReset} disabled={!isDirty || isSaving}>
            Reset
          </Button>
          <Button type="button" onClick={() => void handleSave()} disabled={!isDirty || isSaving}>
            {isSaving ? 'Saving...' : 'Save standards'}
          </Button>
        </div>
      </footer>

      <Toast
        open={toast.open}
        onOpenChange={(open) => setToast((current) => ({ ...current, open }))}
        title={toast.title}
        description={toast.description}
        variant={toast.variant}
      />
    </div>
  );
};

export default Standards;
