import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useLocation } from 'react-router-dom';
import { getConfig, updateConfig, ApiClientError } from '../../api/client';
import type { ApiConfigResponse, ApiConfigUpdateRequest } from '../../api/models';
import { Button, ErrorBanner, LoadingSpinner, Toast } from '../../components';
import { cn } from '../../components/styles';


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

/* ------------------------------------------------------------------ */
/*  Tool-aware field components (never ask users to type tool names)   */
/* ------------------------------------------------------------------ */

/** Reorderable list of enabled tools — ↑↓ buttons, no manual typing. */
function ToolPriorityField({
  value,
  enabledTools,
  onChange,
  helpText,
}: {
  value: string[];
  enabledTools: string[];
  onChange: (v: string[]) => void;
  helpText?: string;
}) {
  // Build the full list: explicitly ordered tools first, then remaining enabled tools in insertion order
  const buildFull = (explicit: string[], all: string[]): string[] => {
    const result = explicit.filter((t) => all.includes(t));
    for (const t of all) if (!result.includes(t)) result.push(t);
    return result;
  };
  const list = buildFull(value, enabledTools);

  const move = (idx: number, dir: -1 | 1) => {
    const next = [...list];
    const target = idx + dir;
    if (target < 0 || target >= next.length) return;
    [next[idx], next[target]] = [next[target], next[idx]];
    onChange(next);
  };

  if (enabledTools.length === 0)
    return (
      <div className="flex flex-col gap-1.5">
        <span className="text-xs font-medium text-[var(--text-secondary)]">Tool Priority</span>
        <p className="text-xs text-[var(--text-muted)] px-2 py-2">No tools enabled.</p>
      </div>
    );

  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-xs font-medium text-[var(--text-secondary)]">Tool Priority</span>
      <div className="rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] p-2 flex flex-col gap-0.5">
        {list.map((tool, idx) => {
          const isExplicit = value.includes(tool);
          return (
            <div
              key={tool}
              className="flex items-center justify-between px-2 py-1 rounded hover:bg-[var(--bg-hover)]"
            >
              <div className="flex items-center gap-2">
                <span
                  className={cn(
                    'text-[10px] tabular-nums w-5 text-right select-none',
                    isExplicit ? 'text-[var(--accent)]' : 'text-[var(--text-muted)]',
                  )}
                >
                  {idx + 1}.
                </span>
                <span
                  className={cn(
                    'text-xs font-mono',
                    isExplicit ? 'text-[var(--text-primary)]' : 'text-[var(--text-muted)]',
                  )}
                >
                  {tool}
                </span>
                {!isExplicit && (
                  <span className="text-[9px] text-[var(--text-muted)] italic">(default)</span>
                )}
              </div>
              <div className="flex gap-1">
                <button
                  onClick={() => move(idx, -1)}
                  disabled={idx === 0}
                  title="Move up"
                  className="w-6 h-5 flex items-center justify-center text-[11px] rounded border border-[var(--border)] bg-[var(--bg-card)] hover:bg-[var(--bg-hover)] disabled:opacity-25 disabled:cursor-not-allowed"
                >
                  ↑
                </button>
                <button
                  onClick={() => move(idx, 1)}
                  disabled={idx === list.length - 1}
                  title="Move down"
                  className="w-6 h-5 flex items-center justify-center text-[11px] rounded border border-[var(--border)] bg-[var(--bg-card)] hover:bg-[var(--bg-hover)] disabled:opacity-25 disabled:cursor-not-allowed"
                >
                  ↓
                </button>
              </div>
            </div>
          );
        })}
      </div>
      {helpText && <span className="text-[10px] text-[var(--text-muted)]">{helpText}</span>}
    </div>
  );
}

/** Per-tool parallel count stepper — no JSON required. */
function MaxParallelPerToolField({
  value,
  enabledTools,
  onChange,
  helpText,
}: {
  value: Record<string, number>;
  enabledTools: string[];
  onChange: (v: Record<string, number>) => void;
  helpText?: string;
}) {
  const adjust = (tool: string, delta: number) => {
    const current = value[tool] ?? 1;
    const next = Math.max(1, current + delta);
    onChange({ ...value, [tool]: next });
  };

  if (enabledTools.length === 0)
    return (
      <div className="flex flex-col gap-1.5">
        <span className="text-xs font-medium text-[var(--text-secondary)]">Max Parallel Per Tool</span>
        <p className="text-xs text-[var(--text-muted)] px-2 py-2">No tools enabled.</p>
      </div>
    );

  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-xs font-medium text-[var(--text-secondary)]">Max Parallel Per Tool</span>
      <div className="rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] p-2 flex flex-col gap-0.5">
        {enabledTools.map((tool) => {
          const count = value[tool] ?? 1;
          return (
            <div
              key={tool}
              className="flex items-center justify-between px-2 py-1 rounded hover:bg-[var(--bg-hover)]"
            >
              <span className="text-xs font-mono text-[var(--text-primary)]">{tool}</span>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => adjust(tool, -1)}
                  disabled={count <= 1}
                  title="Decrease"
                  className="w-6 h-6 flex items-center justify-center text-sm rounded border border-[var(--border)] bg-[var(--bg-card)] hover:bg-[var(--bg-hover)] disabled:opacity-25 disabled:cursor-not-allowed"
                >
                  −
                </button>
                <span className="text-xs tabular-nums w-6 text-center font-medium text-[var(--text-primary)] select-none">
                  {count}
                </span>
                <button
                  onClick={() => adjust(tool, 1)}
                  title="Increase"
                  className="w-6 h-6 flex items-center justify-center text-sm rounded border border-[var(--border)] bg-[var(--bg-card)] hover:bg-[var(--bg-hover)]"
                >
                  +
                </button>
              </div>
            </div>
          );
        })}
      </div>
      {helpText && <span className="text-[10px] text-[var(--text-muted)]">{helpText}</span>}
    </div>
  );
}

/** Category → tools mapping editor — toggleable chips per category, no JSON. */
function ToolSpecializationsField({
  value,
  enabledTools,
  onChange,
  helpText,
}: {
  value: Record<string, string[]>;
  enabledTools: string[];
  onChange: (v: Record<string, string[]>) => void;
  helpText?: string;
}) {
  const [newCat, setNewCat] = useState('');

  const toggleTool = (cat: string, tool: string) => {
    const current = (value[cat] as string[]) ?? [];
    const next = current.includes(tool)
      ? current.filter((t) => t !== tool)
      : [...current, tool];
    onChange({ ...value, [cat]: next });
  };

  const removeCat = (cat: string) => {
    const next = { ...value };
    delete next[cat];
    onChange(next);
  };

  const addCat = () => {
    const trimmed = newCat.trim().toLowerCase().replace(/\s+/g, '_');
    if (!trimmed || Object.prototype.hasOwnProperty.call(value, trimmed)) return;
    onChange({ ...value, [trimmed]: [] });
    setNewCat('');
  };

  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-xs font-medium text-[var(--text-secondary)]">Tool Specializations</span>
      <div className="rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] p-2 flex flex-col gap-2">
        {Object.keys(value).length === 0 && (
          <p className="text-xs text-[var(--text-muted)] px-1 py-0.5">
            No specializations — all tools handle all categories.
          </p>
        )}
        {Object.entries(value).map(([cat, tools]) => (
          <div key={cat} className="rounded border border-[var(--border)] p-2 flex flex-col gap-1.5">
            <div className="flex items-center justify-between">
              <span className="text-[11px] font-mono font-semibold text-[var(--text-primary)]">{cat}</span>
              <button
                onClick={() => removeCat(cat)}
                className="text-[10px] text-[var(--text-muted)] hover:text-[var(--error)] px-1"
                title="Remove category"
              >
                ✕
              </button>
            </div>
            <div className="flex flex-wrap gap-1">
              {enabledTools.length === 0 && (
                <span className="text-[10px] text-[var(--text-muted)]">No tools enabled.</span>
              )}
              {enabledTools.map((tool) => {
                const active = (tools as string[]).includes(tool);
                return (
                  <button
                    key={tool}
                    onClick={() => toggleTool(cat, tool)}
                    className={cn(
                      'px-2 py-0.5 text-[10px] rounded-full border font-mono transition-colors',
                      active
                        ? 'border-[var(--accent)] bg-[var(--accent)]/15 text-[var(--accent)]'
                        : 'border-[var(--border)] bg-[var(--bg-card)] text-[var(--text-muted)] hover:border-[var(--accent)]/50',
                    )}
                  >
                    {tool}
                  </button>
                );
              })}
            </div>
          </div>
        ))}
        {/* Add new category */}
        <div className="flex gap-2 pt-1">
          <input
            type="text"
            className="flex-1 text-xs rounded border border-[var(--border)] bg-[var(--bg-card)] px-2 py-1 text-[var(--text-primary)] outline-none focus:border-[var(--accent)] placeholder-[var(--text-muted)]"
            placeholder="New category (e.g. frontend)"
            value={newCat}
            onChange={(e) => setNewCat(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && addCat()}
          />
          <button
            onClick={addCat}
            disabled={!newCat.trim()}
            className="px-2 py-1 text-xs rounded border border-[var(--border)] bg-[var(--bg-card)] hover:bg-[var(--bg-hover)] text-[var(--text-primary)] disabled:opacity-30"
          >
            + Add
          </button>
        </div>
      </div>
      {helpText && <span className="text-[10px] text-[var(--text-muted)]">{helpText}</span>}
    </div>
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

/* ------------------------------------------------------------------ */
/*  Tab Components & Helpers                                           */
/* ------------------------------------------------------------------ */

function WarningBanner({ message }: { message: string }) {
  return (
    <div className="mb-6 rounded-lg border border-[var(--warning)]/30 bg-[var(--warning)]/10 px-4 py-3 text-xs text-[var(--warning)] flex items-start gap-2">
      <span className="font-semibold select-none">⚠️</span>
      <div>{message}</div>
    </div>
  );
}

function SelectField({
  label,
  value,
  onChange,
  options,
  helpText,
}: {
  label: string;
  value: string | null;
  onChange: (v: string) => void;
  options: { value: string; label: string }[];
  helpText?: string;
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-xs font-medium text-[var(--text-secondary)]">{label}</span>
      <select
        className="rounded-lg border border-[var(--border)] bg-[var(--bg-secondary)] px-3 py-2 text-sm text-[var(--text-primary)] outline-none focus:border-[var(--accent)]"
        value={value ?? ''}
        onChange={(e) => onChange(e.target.value)}
      >
        {options.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
      {helpText && <span className="text-[10px] text-[var(--text-muted)]">{helpText}</span>}
    </label>
  );
}

function PresetSelector({
  onApplyPreset,
}: {
  onApplyPreset: (preset: 'conservative' | 'balanced' | 'throughput') => void;
}) {
  return (
    <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4 mb-6">
      <h4 className="text-xs font-semibold text-[var(--text-primary)] mb-1">Configuration Presets</h4>
      <p className="text-[10px] text-[var(--text-muted)] mb-3">
        Apply predefined performance and safety profiles. This overrides individual settings below.
      </p>
      <div className="flex gap-2">
        <Button
          onClick={() => onApplyPreset('conservative')}
          className="text-xs px-3 py-1.5 border border-[var(--border)] bg-[var(--bg-card)] hover:bg-[var(--bg-hover)] text-[var(--text-primary)]"
        >
          Conservative
        </Button>
        <Button
          onClick={() => onApplyPreset('balanced')}
          className="text-xs px-3 py-1.5 border border-[var(--border)] bg-[var(--bg-card)] hover:bg-[var(--bg-hover)] text-[var(--text-primary)]"
        >
          Balanced
        </Button>
        <Button
          onClick={() => onApplyPreset('throughput')}
          className="text-xs px-3 py-1.5 border border-[var(--border)] bg-[var(--bg-card)] hover:bg-[var(--bg-hover)] text-[var(--text-primary)]"
        >
          Throughput
        </Button>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/*  Tab: Basic settings                                                */
/* ------------------------------------------------------------------ */

function BasicTab({
  draft,
  update,
}: {
  draft: ApiConfigResponse;
  update: (patch: Partial<ApiConfigUpdateRequest>) => void;
}) {
  const handleApplyPreset = (preset: 'conservative' | 'balanced' | 'throughput') => {
    switch (preset) {
      case 'conservative':
        update({
          maxParallel: 1,
          rateLimitFallbackEnabled: false,
          rateLimitThrottleParallel: false,
          mergeAiFix: false,
          safetyPolicy: 'strict',
          destructiveActions: 'double_confirm',
        });
        break;
      case 'balanced':
        update({
          maxParallel: 3,
          rateLimitFallbackEnabled: true,
          rateLimitThrottleParallel: false,
          mergeAiFix: true,
          safetyPolicy: 'standard',
          destructiveActions: 'double_confirm',
        });
        break;
      case 'throughput':
        update({
          maxParallel: 6,
          rateLimitFallbackEnabled: true,
          rateLimitThrottleParallel: true,
          mergeAiFix: true,
          safetyPolicy: 'standard',
          destructiveActions: 'double_confirm',
        });
        break;
    }
  };

  return (
    <div className="flex flex-col gap-6">
      <PresetSelector onApplyPreset={handleApplyPreset} />

      <SectionHeading>General Settings</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <NumberField
          label="Web Interface Port"
          value={draft.webPort}
          onChange={(v) => update({ webPort: v })}
          placeholder="3450"
          helpText="Port the local dashboard Axum server binds to."
        />
        <BooleanField
          label="Offline Mode"
          value={draft.offline}
          onChange={(v) => update({ offline: v })}
          helpText="Prevent all remote network requests and catalog checking."
        />
        <BooleanField
          label="Quiet Mode"
          value={draft.quiet}
          onChange={(v) => update({ quiet: v })}
          helpText="Suppress non-essential stdout output from the CLI."
        />
        <BooleanField
          label="Debug Mode"
          value={draft.debug}
          onChange={(v) => update({ debug: v })}
          helpText="Enable verbose performer logs: full prompt dump, runner line, [MACC] invoke lines. Equivalent to MACC_DEBUG=1 or macc --verbose."
        />
      </div>

      <SectionHeading>Task Execution &amp; Routing</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <SelectField
          label="Coordinator Tool"
          value={draft.coordinatorTool}
          onChange={(v) => update({ coordinatorTool: v || null })}
          options={[
            { value: '', label: '— Auto-select —' },
            ...draft.enabledTools.map((t) => ({ value: t, label: t })),
          ]}
          helpText="AI tool used for coordinator phases (review, fix). Empty = auto-select from enabled tools."
        />
        <NumberField
          label="Max Parallel"
          value={draft.maxParallel}
          onChange={(v) => update({ maxParallel: v })}
          placeholder="3"
          helpText="Maximum tasks the coordinator can run in parallel."
        />
        <NumberField
          label="Timeout (seconds)"
          value={draft.timeoutSeconds}
          onChange={(v) => update({ timeoutSeconds: v })}
          placeholder="0"
          helpText="Global wall-clock timeout (0 is unlimited)."
        />
        <TextField
          label="Reference Branch"
          value={draft.referenceBranch}
          onChange={(v) => update({ referenceBranch: v })}
          placeholder="master"
          helpText="Branch where completed task worktrees are merged."
        />
      </div>

      <SectionHeading>Safety &amp; Confirmation</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <SelectField
          label="Safety Policy"
          value={draft.safetyPolicy ?? null}
          onChange={(v) => update({ safetyPolicy: v })}
          options={[
            { value: 'standard', label: 'Standard (Verify modifications)' },
            { value: 'strict', label: 'Strict (Strict sandbox constraints)' },
          ]}
          helpText="Verification policy applied to tool write operations."
        />
        <SelectField
          label="Destructive Actions"
          value={draft.destructiveActions ?? null}
          onChange={(v) => update({ destructiveActions: v })}
          options={[
            { value: 'double_confirm', label: 'Double Confirm (Prompt twice)' },
            { value: 'single_confirm', label: 'Single Confirm (Prompt once)' },
          ]}
          helpText="Prompt confirmation rules for forced updates/checkouts."
        />
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/*  Tab: Advanced settings                                             */
/* ------------------------------------------------------------------ */

function AdvancedTab({
  draft,
  update,
}: {
  draft: ApiConfigResponse;
  update: (patch: Partial<ApiConfigUpdateRequest>) => void;
}) {
  return (
    <div className="flex flex-col gap-6">
      <WarningBanner message="Advanced Settings: These controls adjust low-level task scheduling, staleness checks, and retry mechanisms. Misconfiguration can affect stability or rate limits." />

      <SectionHeading>Routing &amp; Priorities</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <ToolPriorityField
          value={draft.toolPriority}
          enabledTools={draft.enabledTools}
          onChange={(v) => update({ toolPriority: v })}
          helpText="Drag-free reordering — ↑↓ to change priority. Tools not listed use default order."
        />
        <MaxParallelPerToolField
          value={draft.maxParallelPerTool as Record<string, number>}
          enabledTools={draft.enabledTools}
          onChange={(v) => update({ maxParallelPerTool: v })}
          helpText="Maximum concurrent runs per tool. Default is 1 per tool."
        />
        <ToolSpecializationsField
          value={draft.toolSpecializations as Record<string, string[]>}
          enabledTools={draft.enabledTools}
          onChange={(v) => update({ toolSpecializations: v })}
          helpText="Route task categories to specific tools. Empty = all tools handle all categories."
        />
        <TextField
          label="PRD File"
          value={draft.prdFile}
          onChange={(v) => update({ prdFile: v })}
          placeholder="prd.json"
          helpText="Path to the task sequence definition file."
        />
      </div>

      <SectionHeading>Stale Thresholds</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <NumberField
          label="Stale Claimed (seconds)"
          value={draft.staleClaimedSeconds}
          onChange={(v) => update({ staleClaimedSeconds: v })}
          helpText="Auto-stale timeout for claimed tasks (0 disables)."
        />
        <NumberField
          label="In-Progress Timeout (seconds)"
          value={draft.staleInProgressSeconds}
          onChange={(v) => update({ staleInProgressSeconds: v })}
          helpText="Hard kill timeout for performer processes (0 disables)."
        />
        <NumberField
          label="Stale Changes-Requested (seconds)"
          value={draft.staleChangesRequestedSeconds}
          onChange={(v) => update({ staleChangesRequestedSeconds: v })}
          helpText="Auto-stale timeout for changes-requested tasks (0 disables)."
        />
        <TextField
          label="Stale Action"
          value={draft.staleAction}
          onChange={(v) => update({ staleAction: v })}
          placeholder="block"
          helpText="Action on stale task: block | retry | requeue"
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
          helpText="Maximum retry attempts per task."
        />
      </div>

      <SectionHeading>Rate Limiting</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <NumberField
          label="Backoff Base (seconds)"
          value={draft.rateLimitBackoffBaseSeconds}
          onChange={(v) => update({ rateLimitBackoffBaseSeconds: v })}
          placeholder="30"
          helpText="Initial E601 backoff delay."
        />
        <NumberField
          label="Backoff Max (seconds)"
          value={draft.rateLimitBackoffMaxSeconds}
          onChange={(v) => update({ rateLimitBackoffMaxSeconds: v })}
          placeholder="300"
          helpText="Backoff cap for exponential growth."
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

      <SectionHeading>Merge Behavior</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
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
          helpText="Timeout for AI merge-fix hook execution."
        />
      </div>

      <SectionHeading>Process Lifecycle</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <NumberField
          label="Ghost Heartbeat Grace (seconds)"
          value={draft.ghostHeartbeatGraceSeconds}
          onChange={(v) => update({ ghostHeartbeatGraceSeconds: v })}
          placeholder="30"
          helpText="Grace period before treating a dead process as a ghost."
        />
        <NumberField
          label="Dispatch Cooldown (seconds)"
          value={draft.dispatchCooldownSeconds}
          onChange={(v) => update({ dispatchCooldownSeconds: v })}
          placeholder="2"
          helpText="Wait between dispatch cycles."
        />
        <NumberField
          label="Force-Kill Grace (seconds)"
          value={draft.forceKillGraceSeconds}
          onChange={(v) => update({ forceKillGraceSeconds: v })}
          helpText="Wait after IPC failure before force-killing a performer."
        />
        <NumberField
          label="Max Review Cycles"
          value={draft.maxReviewCycles ?? null}
          onChange={(v) => update({ maxReviewCycles: v })}
          helpText="Max review loop cycles per task (0 to skip review)."
        />
        <NumberField
          label="Max Dispatch"
          value={draft.maxDispatch}
          onChange={(v) => update({ maxDispatch: v })}
          placeholder="10"
          helpText="Maximum tasks dispatched per run (0 is unlimited)."
        />
        <NumberField
          label="Phase Runner Max Attempts"
          value={draft.phaseRunnerMaxAttempts}
          onChange={(v) => update({ phaseRunnerMaxAttempts: v })}
          placeholder="1"
          helpText="Max attempts for phase runner fallback."
        />
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/*  Tab: Admin settings                                                */
/* ------------------------------------------------------------------ */

function AdminTab({
  draft,
  update,
}: {
  draft: ApiConfigResponse;
  update: (patch: Partial<ApiConfigUpdateRequest>) => void;
}) {
  return (
    <div className="flex flex-col gap-6">
      <WarningBanner message="Administrative Settings: These settings control raw storage engines, migration layers, and low-level runtime gates. Modifying these is recommended only for systems administrators or developers debugging internal runner state." />

      <SectionHeading>Database &amp; Storage</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <TextField
          label="Storage Mode"
          value={draft.storageMode}
          onChange={(v) => update({ storageMode: v })}
          placeholder="json"
          helpText="Coordinator storage engine mode: json | sqlite"
        />
        <TextField
          label="Task Registry File"
          value={draft.taskRegistryFile}
          onChange={(v) => update({ taskRegistryFile: v })}
          placeholder=".macc/automation/task/task_registry.json"
          helpText="Underlying file path for local task registry state."
        />
      </div>

      <SectionHeading>Flush Behavior</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <NumberField
          label="Log Flush Lines"
          value={draft.logFlushLines}
          onChange={(v) => update({ logFlushLines: v })}
          helpText="Flush logs every N lines. 0 uses runtime default."
        />
        <NumberField
          label="Log Flush Milliseconds"
          value={draft.logFlushMs}
          onChange={(v) => update({ logFlushMs: v })}
          helpText="Flush logs every N ms. 0 uses runtime default."
        />
      </div>

      <SectionHeading>Compatibility Flags</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
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
          helpText="Fallback to JSON registry if SQLite is corrupted."
        />
        <NumberField
          label="Mirror JSON Debounce (ms)"
          value={draft.mirrorJsonDebounceMs}
          onChange={(v) => update({ mirrorJsonDebounceMs: v })}
          helpText="Debounce SQLite-to-JSON compatibility export."
        />
      </div>

      <SectionHeading>Cutover Controls</SectionHeading>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <NumberField
          label="Cutover Gate Window Events"
          value={draft.cutoverGateWindowEvents}
          onChange={(v) => update({ cutoverGateWindowEvents: v })}
          placeholder="2000"
          helpText="Recent events inspected by cutover gate to assess storage health."
        />
        <NumberField
          label="Max Blocked Ratio"
          value={draft.cutoverGateMaxBlockedRatio}
          onChange={(v) => update({ cutoverGateMaxBlockedRatio: v })}
          placeholder="0.25"
          helpText="Maximum ratio of blocked events allowed before cutover block."
        />
        <NumberField
          label="Max Stale Ratio"
          value={draft.cutoverGateMaxStaleRatio}
          onChange={(v) => update({ cutoverGateMaxStaleRatio: v })}
          placeholder="0.25"
          helpText="Maximum ratio of stale events allowed before cutover block."
        />
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/*  Tab: Raw (raw JSON editor)                                         */
/* ------------------------------------------------------------------ */

function RawTab({
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

type SettingsTab = 'basic' | 'advanced' | 'admin' | 'raw';

const TABS: { key: SettingsTab; label: string }[] = [
  { key: 'basic', label: 'Basic' },
  { key: 'advanced', label: 'Advanced' },
  { key: 'admin', label: 'Admin' },
  { key: 'raw', label: 'Raw JSON' },
];

const Settings: React.FC = () => {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [config, setConfig] = useState<ApiConfigResponse | null>(null);
  const [draft, setDraft] = useState<ApiConfigResponse | null>(null);
  const [activeTab, setActiveTab] = useState<SettingsTab>('basic');
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

    const basicKeys = [
      'webPort',
      'offline',
      'quiet',
      'coordinatorTool',
      'maxParallel',
      'timeoutSeconds',
      'safetyPolicy',
      'destructiveActions',
      'referenceBranch',
    ];
    const advancedKeys = [
      'prdFile',
      'toolPriority',
      'maxParallelPerTool',
      'toolSpecializations',
      'maxDispatch',
      'phaseRunnerMaxAttempts',
      'dispatchCooldownSeconds',
      'staleClaimedSeconds',
      'staleInProgressSeconds',
      'staleChangesRequestedSeconds',
      'staleAction',
      'mergeAiFix',
      'mergeJobTimeoutSeconds',
      'mergeHookTimeoutSeconds',
      'ghostHeartbeatGraceSeconds',
      'errorCodeRetryList',
      'errorCodeRetryMax',
      'rateLimitBackoffBaseSeconds',
      'rateLimitBackoffMaxSeconds',
      'rateLimitFallbackEnabled',
      'rateLimitThrottleParallel',
      'forceKillGraceSeconds',
      'maxReviewCycles',
    ];
    const adminKeys = [
      'storageMode',
      'taskRegistryFile',
      'logFlushLines',
      'logFlushMs',
      'mirrorJsonDebounceMs',
      'jsonCompat',
      'legacyJsonFallback',
      'cutoverGateWindowEvents',
      'cutoverGateMaxBlockedRatio',
      'cutoverGateMaxStaleRatio',
    ];

    const key = state.highlightSettingKey;
    if (basicKeys.some((prefix) => key.startsWith(prefix))) {
      setActiveTab('basic');
      return;
    }

    if (advancedKeys.some((prefix) => key.startsWith(prefix))) {
      setActiveTab('advanced');
      return;
    }

    if (adminKeys.some((prefix) => key.startsWith(prefix))) {
      setActiveTab('admin');
      return;
    }

    setActiveTab('raw');
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
            Basic, advanced, and administrative configuration.
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
        {activeTab === 'basic' && <BasicTab draft={draft} update={updateDraft} />}
        {activeTab === 'advanced' && <AdvancedTab draft={draft} update={updateDraft} />}
        {activeTab === 'admin' && <AdminTab draft={draft} update={updateDraft} />}
        {activeTab === 'raw' && <RawTab config={draft} onApplyRaw={handleApplyRaw} />}
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
