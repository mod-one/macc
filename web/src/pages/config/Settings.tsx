import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useLocation } from 'react-router-dom';
import { getConfig, updateConfig, ApiClientError } from '../../api/client';
import type { ApiConfigResponse, ApiConfigUpdateRequest } from '../../api/models';
import { Button, ErrorBanner, LoadingSpinner, Toast } from '../../components';
import { cn } from '../../components/styles';

type SettingsTab = 'general' | 'coordinator' | 'advanced';

interface ToastState {
  open: boolean;
  title: string;
  description?: string;
  variant: 'success' | 'error' | 'warning';
}

function formatError(error: unknown): string {
  if (error instanceof ApiClientError) {
    return `${error.envelope.error.message} (${error.envelope.error.code})`;
  }
  if (error instanceof Error) return error.message;
  return 'Unexpected error.';
}

function ensureStringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === 'string') : [];
}

function ensureRecord<T>(value: unknown): Record<string, T> {
  return value !== null && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, T>) : {};
}

function normalizeConfigResponse(config: ApiConfigResponse): ApiConfigResponse {
  return {
    ...config,
    enabledTools: ensureStringArray(config.enabledTools),
    selectedSkills: ensureStringArray(config.selectedSkills),
    selectedAgents: ensureStringArray(config.selectedAgents),
    selectedMcp: ensureStringArray(config.selectedMcp),
    toolPriority: ensureStringArray(config.toolPriority),
    managedEnvironmentWarnings: ensureStringArray(config.managedEnvironmentWarnings),
    toolConfig: ensureRecord(config.toolConfig),
    toolSettings: ensureRecord(config.toolSettings),
    standardsInline: ensureRecord(config.standardsInline),
    maxParallelPerTool: ensureRecord(config.maxParallelPerTool),
    toolSpecializations: ensureRecord(config.toolSpecializations),
  };
}

/* ------------------------------------------------------------------ */
/*  Field helpers                                                      */
/* ------------------------------------------------------------------ */

function NumberField({
  label,
  value,
  onChange,
  placeholder,
  helpText,
}: {
  label: string;
  value: number | null;
  onChange: (v: number | null) => void;
  placeholder?: string;
  helpText?: string;
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-xs font-medium text-[var(--text-secondary)]">{label}</span>
      <input
        type="number"
        className="rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-2 text-sm text-[var(--text-primary)] outline-none focus:border-[var(--accent)]"
        value={value ?? ''}
        onChange={(e) => onChange(e.target.value === '' ? null : Number(e.target.value))}
        placeholder={placeholder}
      />
      {helpText && <span className="text-[10px] text-[var(--text-muted)]">{helpText}</span>}
    </label>
  );
}

function TextField({
  label,
  value,
  onChange,
  placeholder,
  helpText,
}: {
  label: string;
  value: string | null;
  onChange: (v: string | null) => void;
  placeholder?: string;
  helpText?: string;
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-xs font-medium text-[var(--text-secondary)]">{label}</span>
      <input
        type="text"
        className="rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-2 text-sm text-[var(--text-primary)] outline-none focus:border-[var(--accent)]"
        value={value ?? ''}
        onChange={(e) => onChange(e.target.value === '' ? null : e.target.value)}
        placeholder={placeholder}
      />
      {helpText && <span className="text-[10px] text-[var(--text-muted)]">{helpText}</span>}
    </label>
  );
}

function JsonObjectField({
  label,
  value,
  onChange,
  placeholder,
  helpText,
}: {
  label: string;
  value: Record<string, unknown>;
  onChange: (value: Record<string, unknown>) => void;
  placeholder?: string;
  helpText?: string;
}) {
  const [rawText, setRawText] = useState(() => JSON.stringify(value, null, 2));
  const [parseError, setParseError] = useState<string | null>(null);
  const previousValueRef = useRef(value);

  useEffect(() => {
    if (previousValueRef.current === value) {
      return;
    }
    previousValueRef.current = value;
    setRawText(JSON.stringify(value, null, 2));
    setParseError(null);
  }, [value]);

  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-xs font-medium text-[var(--text-secondary)]">{label}</span>
      <textarea
        className={cn(
          'min-h-[132px] rounded-lg border bg-[var(--bg-secondary)] px-3 py-2 font-mono text-xs text-[var(--text-primary)] outline-none focus:border-[var(--accent)]',
          parseError ? 'border-[var(--error)]' : 'border-[var(--border)]',
        )}
        value={rawText}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(e) => {
          const nextText = e.target.value;
          setRawText(nextText);

          if (nextText.trim() === '') {
            setParseError(null);
            onChange({});
            return;
          }

          try {
            const parsed = JSON.parse(nextText) as unknown;
            if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
              setParseError('Value must be a JSON object.');
              return;
            }
            setParseError(null);
            onChange(parsed as Record<string, unknown>);
          } catch (error) {
            setParseError(error instanceof Error ? error.message : 'Invalid JSON');
          }
        }}
      />
      {helpText && <span className="text-[10px] text-[var(--text-muted)]">{helpText}</span>}
      {parseError && <span className="text-[10px] text-[var(--error)]">{parseError}</span>}
    </label>
  );
}

function BooleanField({
  label,
  value,
  onChange,
  helpText,
}: {
  label: string;
  value: boolean | null;
  onChange: (v: boolean) => void;
  helpText?: string;
}) {
  return (
    <label className="flex items-center gap-3">
      <input
        type="checkbox"
        className="h-4 w-4 accent-[var(--accent)]"
        checked={value ?? false}
        onChange={(e) => onChange(e.target.checked)}
      />
      <div className="flex flex-col">
        <span className="text-sm text-[var(--text-primary)]">{label}</span>
        {helpText && <span className="text-[10px] text-[var(--text-muted)]">{helpText}</span>}
      </div>
    </label>
  );
}

function SectionHeading({ children }: { children: React.ReactNode }) {
  return (
    <h3 className="mb-3 border-b border-[var(--border)] pb-2 text-sm font-semibold text-[var(--text-primary)]">
      {children}
    </h3>
  );
}

/* ------------------------------------------------------------------ */
/*  Tab: General                                                       */
/* ------------------------------------------------------------------ */

function GeneralTab({
  draft,
  update,
}: {
  draft: ApiConfigResponse;
  update: (patch: Partial<ApiConfigUpdateRequest>) => void;
}) {
  return (
    <div className="flex flex-col gap-6">
      <SectionHeading>General</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <NumberField
          label="Web Port"
          value={draft.webPort}
          onChange={(v) => update({ webPort: v })}
          placeholder="3450"
          helpText="Port for the web interface."
        />
        <BooleanField
          label="Offline Mode"
          value={draft.offline}
          onChange={(v) => update({ offline: v })}
          helpText="Disable all network operations."
        />
        <BooleanField
          label="Quiet Mode"
          value={draft.quiet}
          onChange={(v) => update({ quiet: v })}
          helpText="Suppress non-essential output."
        />
      </div>

      <SectionHeading>Reference</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <TextField
          label="Reference Branch"
          value={draft.referenceBranch}
          onChange={(v) => update({ referenceBranch: v })}
          placeholder="main"
        />
        <TextField
          label="PRD File"
          value={draft.prdFile}
          onChange={(v) => update({ prdFile: v })}
          placeholder="prd.json"
        />
        <TextField
          label="Coordinator Tool"
          value={draft.coordinatorTool}
          onChange={(v) => update({ coordinatorTool: v })}
          placeholder="claude"
        />
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/*  Tab: Coordinator                                                   */
/* ------------------------------------------------------------------ */

function CoordinatorTab({
  draft,
  update,
}: {
  draft: ApiConfigResponse;
  update: (patch: Partial<ApiConfigUpdateRequest>) => void;
}) {
  return (
    <div className="flex flex-col gap-6">
      <SectionHeading>Coordinator Routing</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <JsonObjectField
          label="Max Parallel Per Tool"
          value={draft.maxParallelPerTool}
          onChange={(value) => update({ maxParallelPerTool: value as Record<string, number> })}
          placeholder='{"claude": 2, "codex": 1}'
          helpText="Per-tool concurrency caps as a JSON object."
        />
        <JsonObjectField
          label="Tool Specializations"
          value={draft.toolSpecializations}
          onChange={(value) => update({ toolSpecializations: value as Record<string, string[]> })}
          placeholder='{"frontend": ["claude", "codex"]}'
          helpText="Category routing as a JSON object mapping category to tool lists."
        />
      </div>

      <SectionHeading>Dispatch &amp; Parallelism</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <NumberField
          label="Max Dispatch"
          value={draft.maxDispatch}
          onChange={(v) => update({ maxDispatch: v })}
          placeholder="10"
          helpText="Maximum tasks dispatched per run. Empty uses default 10. 0 means no cap."
        />
        <NumberField
          label="Max Parallel"
          value={draft.maxParallel}
          onChange={(v) => update({ maxParallel: v })}
          placeholder="3"
          helpText="Maximum concurrent workers. Empty uses default 3."
        />
        <NumberField
          label="Timeout (seconds)"
          value={draft.timeoutSeconds}
          onChange={(v) => update({ timeoutSeconds: v })}
          placeholder="0"
          helpText="Task execution timeout. 0 disables timeout."
        />
        <NumberField
          label="Phase Runner Max Attempts"
          value={draft.phaseRunnerMaxAttempts}
          onChange={(v) => update({ phaseRunnerMaxAttempts: v })}
          placeholder="1"
          helpText="Max attempts for phase runner fallback. Empty uses default 1."
        />
        <NumberField
          label="Dispatch Cooldown (seconds)"
          value={draft.dispatchCooldownSeconds}
          onChange={(v) => update({ dispatchCooldownSeconds: v })}
          placeholder="2"
          helpText="Wait between dispatch cycles. Empty uses default 2."
        />
      </div>

      <SectionHeading>Stale Thresholds</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <NumberField
          label="Stale Claimed (seconds)"
          value={draft.staleClaimedSeconds}
          onChange={(v) => update({ staleClaimedSeconds: v })}
        />
        <NumberField
          label="Stale In-Progress (seconds)"
          value={draft.staleInProgressSeconds}
          onChange={(v) => update({ staleInProgressSeconds: v })}
        />
        <NumberField
          label="Stale Changes-Requested (seconds)"
          value={draft.staleChangesRequestedSeconds}
          onChange={(v) => update({ staleChangesRequestedSeconds: v })}
        />
        <TextField
          label="Stale Action"
          value={draft.staleAction}
          onChange={(v) => update({ staleAction: v })}
          placeholder="block"
          helpText="Action on stale task: block | retry | requeue"
        />
      </div>

      <SectionHeading>Coordinator Runtime</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <NumberField
          label="Log Flush Lines"
          value={draft.logFlushLines}
          onChange={(v) => update({ logFlushLines: v })}
          helpText="Flush coordinator logs every N lines. 0 uses runtime default."
        />
        <NumberField
          label="Log Flush Milliseconds"
          value={draft.logFlushMs}
          onChange={(v) => update({ logFlushMs: v })}
          helpText="Flush coordinator logs every N milliseconds. 0 uses runtime default."
        />
        <NumberField
          label="Mirror JSON Debounce (ms)"
          value={draft.mirrorJsonDebounceMs}
          onChange={(v) => update({ mirrorJsonDebounceMs: v })}
          helpText="Debounce SQLite-to-JSON compatibility export. 0 disables debounce."
        />
        <BooleanField
          label="Merge AI Fix"
          value={draft.mergeAiFix}
          onChange={(v) => update({ mergeAiFix: v })}
          helpText="Enable AI-driven resolution for merge conflicts."
        />
        <NumberField
          label="Merge Job Timeout (seconds)"
          value={draft.mergeJobTimeoutSeconds}
          onChange={(v) => update({ mergeJobTimeoutSeconds: v })}
          helpText="Timeout for git merge operations."
        />
        <NumberField
          label="Merge Hook Timeout (seconds)"
          value={draft.mergeHookTimeoutSeconds}
          onChange={(v) => update({ mergeHookTimeoutSeconds: v })}
          placeholder="90"
          helpText="Timeout for the AI merge-fix hook. Empty uses default 90."
        />
        <NumberField
          label="Ghost Heartbeat Grace (seconds)"
          value={draft.ghostHeartbeatGraceSeconds}
          onChange={(v) => update({ ghostHeartbeatGraceSeconds: v })}
          placeholder="30"
          helpText="Grace period before a dead process is treated as a ghost. Empty uses default 30."
        />
        <BooleanField
          label="JSON Compatibility"
          value={draft.jsonCompat}
          onChange={(v) => update({ jsonCompat: v })}
          helpText="Enable JSON snapshot export for external tool compatibility."
        />
        <BooleanField
          label="Legacy JSON Fallback"
          value={draft.legacyJsonFallback}
          onChange={(v) => update({ legacyJsonFallback: v })}
          helpText="Fallback to the JSON task registry if SQLite is missing or corrupted."
        />
        <NumberField
          label="Force-Kill Grace (seconds)"
          value={draft.forceKillGraceSeconds ?? null}
          onChange={(v) => update({ forceKillGraceSeconds: v === null ? null : Math.max(0, v) })}
          helpText="Wait after IPC failure before force-killing a performer."
        />
      </div>

      <SectionHeading>Error Retry</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <TextField
          label="Error Code Retry List"
          value={draft.errorCodeRetryList}
          onChange={(v) => update({ errorCodeRetryList: v })}
          placeholder="E101,E102,E103,E301,E302,E303,E601,E603"
          helpText="Comma-separated error codes eligible for auto-retry."
        />
        <NumberField
          label="Error Code Retry Max"
          value={draft.errorCodeRetryMax}
          onChange={(v) => update({ errorCodeRetryMax: v })}
          placeholder="2"
          helpText="Maximum retry attempts per task. Empty uses default 2."
        />
      </div>

      <SectionHeading>Rate Limit</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <NumberField
          label="Backoff Base (seconds)"
          value={draft.rateLimitBackoffBaseSeconds}
          onChange={(v) => update({ rateLimitBackoffBaseSeconds: v })}
          placeholder="30"
          helpText="Initial E601 backoff delay. Empty uses default 30."
        />
        <NumberField
          label="Backoff Max (seconds)"
          value={draft.rateLimitBackoffMaxSeconds}
          onChange={(v) => update({ rateLimitBackoffMaxSeconds: v })}
          placeholder="300"
          helpText="Backoff cap for E601 exponential growth. Empty uses default 300."
        />
        <BooleanField
          label="Fallback Enabled"
          value={draft.rateLimitFallbackEnabled}
          onChange={(v) => update({ rateLimitFallbackEnabled: v })}
          helpText="Fall back to next tool on rate-limit."
        />
        <BooleanField
          label="Throttle Parallel"
          value={draft.rateLimitThrottleParallel}
          onChange={(v) => update({ rateLimitThrottleParallel: v })}
          helpText="Reduce concurrency on rate-limit."
        />
      </div>

      <SectionHeading>Tool Priority</SectionHeading>
      <div className="grid grid-cols-1 gap-4">
        <TextField
          label="Tool Priority"
          value={draft.toolPriority.length > 0 ? draft.toolPriority.join(', ') : null}
          onChange={(v) =>
            update({
              toolPriority: v
                ? v.split(',').map((s) => s.trim()).filter(Boolean)
                : [],
            })
          }
          placeholder="claude, codex, gemini"
          helpText="Comma-separated list of tools in priority order."
        />
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/*  Tab: Advanced (raw JSON editor)                                    */
/* ------------------------------------------------------------------ */

function AdvancedTab({
  config,
  onApplyRaw,
}: {
  config: ApiConfigResponse;
  onApplyRaw: (raw: string) => void;
}) {
  const [rawText, setRawText] = useState(() => JSON.stringify(config, null, 2));
  const [parseError, setParseError] = useState<string | null>(null);
  const [prevConfig, setPrevConfig] = useState(config);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Sync rawText when config reference changes (React-idiomatic state adjustment during render)
  if (config !== prevConfig) {
    setPrevConfig(config);
    setRawText(JSON.stringify(config, null, 2));
    setParseError(null);
  }

  const handleApply = useCallback(() => {
    try {
      JSON.parse(rawText);
      setParseError(null);
      onApplyRaw(rawText);
    } catch (e) {
      setParseError(e instanceof Error ? e.message : 'Invalid JSON');
    }
  }, [rawText, onApplyRaw]);

  return (
    <div className="flex flex-col gap-4">
      <SectionHeading>Raw Configuration (JSON)</SectionHeading>
      <p className="text-xs text-[var(--text-muted)]">
        Edit the full configuration as JSON. Changes apply when you click Save.
      </p>
      {parseError && <ErrorBanner message={parseError} />}
      <textarea
        ref={textareaRef}
        className="min-h-[420px] w-full rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] p-4 font-mono text-xs leading-relaxed text-[var(--text-primary)] outline-none focus:border-[var(--accent)]"
        value={rawText}
        onChange={(e) => {
          setRawText(e.target.value);
          setParseError(null);
        }}
        spellCheck={false}
      />
      <div className="flex justify-end">
        <Button
          onClick={handleApply}
          className="rounded-lg bg-[var(--accent)] px-4 py-2 text-xs font-medium text-white transition-colors hover:opacity-90"
        >
          Apply JSON
        </Button>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/*  Main Settings Page                                                 */
/* ------------------------------------------------------------------ */

const TABS: { key: SettingsTab; label: string }[] = [
  { key: 'general', label: 'General' },
  { key: 'coordinator', label: 'Coordinator' },
  { key: 'advanced', label: 'Advanced' },
];

const Settings: React.FC = () => {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [config, setConfig] = useState<ApiConfigResponse | null>(null);
  const [draft, setDraft] = useState<ApiConfigResponse | null>(null);
  const [activeTab, setActiveTab] = useState<SettingsTab>('general');
  const [focusedSettingKey, setFocusedSettingKey] = useState<string | null>(null);
  const [toast, setToast] = useState<ToastState>({ open: false, title: '', variant: 'success' });
  const location = useLocation();

  const isDirty = config !== null && draft !== null && JSON.stringify(config) !== JSON.stringify(draft);

  /* Load config */
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    getConfig()
      .then((res) => {
        if (cancelled) return;
        const normalized = normalizeConfigResponse(res);
        setConfig(normalized);
        setDraft(normalized);
      })
      .catch((err) => {
        if (cancelled) return;
        setError(formatError(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const state = location.state as { highlightSettingKey?: string } | null;
    if (!state?.highlightSettingKey) {
      return;
    }

    setFocusedSettingKey(state.highlightSettingKey);

    const generalKeys = [
      'webPort',
      'offline',
      'quiet',
      'referenceBranch',
      'prdFile',
      'coordinatorTool',
      'enabledTools',
      'selectedSkills',
      'selectedAgents',
      'selectedMcp',
      'toolConfig',
      'toolSettings',
      'standardsPath',
      'standardsInline',
      'toolPriority',
    ];
    const coordinatorKeys = [
      'maxParallelPerTool',
      'toolSpecializations',
      'maxDispatch',
      'maxParallel',
      'timeoutSeconds',
      'phaseRunnerMaxAttempts',
      'logFlushLines',
      'logFlushMs',
      'mirrorJsonDebounceMs',
      'mergeAiFix',
      'mergeJobTimeoutSeconds',
      'mergeHookTimeoutSeconds',
      'ghostHeartbeatGraceSeconds',
      'dispatchCooldownSeconds',
      'staleClaimedSeconds',
      'staleInProgressSeconds',
      'staleChangesRequestedSeconds',
      'staleAction',
      'jsonCompat',
      'legacyJsonFallback',
      'errorCodeRetryList',
      'errorCodeRetryMax',
      'rateLimitBackoffBaseSeconds',
      'rateLimitBackoffMaxSeconds',
      'rateLimitFallbackEnabled',
      'rateLimitThrottleParallel',
      'forceKillGraceSeconds',
    ];

    if (generalKeys.some((prefix) => state.highlightSettingKey?.startsWith(prefix))) {
      setActiveTab('general');
      return;
    }

    if (coordinatorKeys.some((prefix) => state.highlightSettingKey?.startsWith(prefix))) {
      setActiveTab('coordinator');
      return;
    }

    setActiveTab('advanced');
  }, [location.state]);

  /* Patch draft */
  const updateDraft = useCallback((patch: Partial<ApiConfigUpdateRequest>) => {
    setDraft((prev) => (prev ? { ...prev, ...patch } : prev));
  }, []);

  /* Discard */
  const handleDiscard = useCallback(() => {
    setDraft(config);
  }, [config]);

  /* Save */
  const handleSave = useCallback(async () => {
    if (!draft) return;
    setSaving(true);
    try {
      const updated = normalizeConfigResponse(await updateConfig(draft as ApiConfigUpdateRequest));
      setConfig(updated);
      setDraft(updated);
      setToast({ open: true, title: 'Settings saved', variant: 'success' });
    } catch (err) {
      setToast({
        open: true,
        title: 'Failed to save',
        description: formatError(err),
        variant: 'error',
      });
    } finally {
      setSaving(false);
    }
  }, [draft]);

  /* Apply raw JSON from Advanced tab */
  const handleApplyRaw = useCallback(
    (raw: string) => {
      try {
        const parsed = normalizeConfigResponse(JSON.parse(raw) as ApiConfigResponse);
        setDraft(parsed);
      } catch {
        /* validation handled in AdvancedTab */
      }
    },
    [],
  );

  if (loading) {
    return (
      <div className="flex items-center justify-center py-20">
        <LoadingSpinner label="Loading settings..." />
      </div>
    );
  }

  if (error || !draft || !config) {
    return (
      <div className="flex flex-col gap-6 p-6">
        <ErrorBanner message={error ?? 'Failed to load configuration.'} />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      {/* Header */}
      <header className="flex items-center justify-between rounded-[2rem] border border-[var(--border)] bg-[var(--bg-card)] p-6 shadow-[var(--shadow-soft)]">
        <div>
          <h1 className="text-3xl font-semibold tracking-tight text-[var(--text-primary)]">Settings</h1>
          <p className="mt-1 text-sm text-[var(--text-secondary)]">
            General, coordinator, and advanced configuration.
          </p>
          {focusedSettingKey && (
            <p className="mt-3 inline-flex rounded-full border border-[var(--accent)]/30 bg-[var(--accent)]/10 px-3 py-1 text-xs font-medium text-[var(--accent)]">
              Focused key: {focusedSettingKey}
            </p>
          )}
        </div>
        <div className="flex items-center gap-2">
          {isDirty && (
            <Button
              onClick={handleDiscard}
              className="rounded-lg border border-[var(--border)] bg-transparent px-4 py-2 text-xs font-medium text-[var(--text-secondary)] transition-colors hover:bg-white/5"
            >
              Discard
            </Button>
          )}
          <Button
            onClick={handleSave}
            disabled={!isDirty || saving}
            className={cn(
              'rounded-lg px-4 py-2 text-xs font-medium text-white transition-colors',
              isDirty ? 'bg-[var(--accent)] hover:opacity-90' : 'bg-[var(--accent)]/50 cursor-not-allowed opacity-50',
            )}
          >
            {saving ? 'Saving...' : 'Save'}
          </Button>
        </div>
      </header>

      {/* Tabs */}
      <div className="flex rounded-lg border border-[var(--border)] overflow-hidden">
        {TABS.map((tab) => (
          <button
            key={tab.key}
            onClick={() => setActiveTab(tab.key)}
            className={cn(
              'flex items-center gap-1.5 px-4 py-2.5 text-xs font-medium transition-colors',
              activeTab === tab.key
                ? 'bg-[var(--accent)] text-white'
                : 'bg-[var(--bg-secondary)] text-[var(--text-muted)] hover:bg-[var(--bg-hover)]',
            )}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Unsaved indicator */}
      {isDirty && (
        <div className="rounded-lg border border-[var(--warning)]/30 bg-[var(--warning)]/10 px-4 py-2 text-xs text-[var(--warning)]">
          You have unsaved changes.
        </div>
      )}

      {/* Tab content */}
      <div className="rounded-[var(--radius-card)] border border-[var(--border)] bg-[var(--bg-card)] p-6 shadow-[var(--shadow-soft)]">
        {activeTab === 'general' && <GeneralTab draft={draft} update={updateDraft} />}
        {activeTab === 'coordinator' && <CoordinatorTab draft={draft} update={updateDraft} />}
        {activeTab === 'advanced' && <AdvancedTab config={draft} onApplyRaw={handleApplyRaw} />}
      </div>

      {/* Toast */}
      <Toast
        open={toast.open}
        onOpenChange={(open) => setToast((prev) => ({ ...prev, open }))}
        title={toast.title}
        description={toast.description}
        variant={toast.variant}
      />
    </div>
  );
};

export default Settings;
