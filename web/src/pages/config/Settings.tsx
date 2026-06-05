import React, { useCallback, useEffect, useState } from 'react';
import { useLocation } from 'react-router-dom';
import { getConfig, updateConfig, ApiClientError } from '../../api/client';
import type { ApiConfigResponse, ApiConfigUpdateRequest } from '../../api/models';
import { ErrorBanner, LoadingSpinner, Toast } from '../../components';

/* ── Types ───────────────────────────────────────────────────── */
interface ToastState {
  open: boolean;
  title: string;
  description?: string;
  variant: 'success' | 'error' | 'warning';
}

type SettingsSection = 'basic' | 'advanced' | 'admin' | 'raw';

/* ── Helpers ─────────────────────────────────────────────────── */
function formatError(error: unknown): string {
  if (error instanceof ApiClientError) return `${error.envelope.error.message} (${error.envelope.error.code})`;
  if (error instanceof Error) return error.message;
  return 'Unexpected error.';
}

function ensureStringArray(v: unknown): string[] {
  return Array.isArray(v) ? v.filter((e): e is string => typeof e === 'string') : [];
}

function ensureRecord<T>(v: unknown): Record<string, T> {
  return v !== null && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, T>) : {};
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

/* ── Base field styles ───────────────────────────────────────── */
const inputStyle: React.CSSProperties = {
  height: 32,
  borderRadius: 'var(--radius-md)',
  border: '1px solid var(--border)',
  background: 'var(--bg-secondary)',
  color: 'var(--text-primary)',
  fontSize: 'var(--text-sm)',
  padding: '0 10px',
  outline: 'none',
  width: '100%',
  transition: 'border-color 100ms',
  fontFamily: 'var(--font-ui)',
};

/* ── Field primitives ────────────────────────────────────────── */
function Label({ children }: { children: React.ReactNode }) {
  return (
    <span style={{ fontSize: '11px', fontWeight: 500, color: 'var(--text-muted)', display: 'block', marginBottom: 5 }}>
      {children}
    </span>
  );
}

function Help({ children }: { children: React.ReactNode }) {
  return (
    <span style={{ fontSize: '10px', color: 'var(--text-muted)', display: 'block', marginTop: 4, lineHeight: 1.45 }}>
      {children}
    </span>
  );
}

function Field({ label, help, children }: { label: string; help?: string; children: React.ReactNode }) {
  return (
    <div>
      <Label>{label}</Label>
      {children}
      {help && <Help>{help}</Help>}
    </div>
  );
}

function NumberInput({ value, onChange, placeholder }: { value: number | null; onChange: (v: number | null) => void; placeholder?: string }) {
  return (
    <input
      type="number"
      style={inputStyle}
      value={value ?? ''}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value === '' ? null : Number(e.target.value))}
      onFocus={(e) => { (e.target as HTMLInputElement).style.borderColor = 'var(--accent)'; }}
      onBlur={(e) => { (e.target as HTMLInputElement).style.borderColor = 'var(--border)'; }}
    />
  );
}

function TextInput({ value, onChange, placeholder, mono }: { value: string | null; onChange: (v: string | null) => void; placeholder?: string; mono?: boolean }) {
  return (
    <input
      type="text"
      style={{ ...inputStyle, fontFamily: mono ? 'var(--font-mono)' : 'var(--font-ui)', fontSize: mono ? '11px' : 'var(--text-sm)' }}
      value={value ?? ''}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value === '' ? null : e.target.value)}
      onFocus={(e) => { (e.target as HTMLInputElement).style.borderColor = 'var(--accent)'; }}
      onBlur={(e) => { (e.target as HTMLInputElement).style.borderColor = 'var(--border)'; }}
    />
  );
}

function SelectInput({ value, onChange, options }: { value: string | null; onChange: (v: string) => void; options: { value: string; label: string }[] }) {
  return (
    <select
      style={{ ...inputStyle, cursor: 'pointer', paddingRight: 28, appearance: 'none',
        backgroundImage: `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%23737373' stroke-width='2'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E")`,
        backgroundRepeat: 'no-repeat', backgroundPosition: 'right 8px center' }}
      value={value ?? ''}
      onChange={(e) => onChange(e.target.value)}
      onFocus={(e) => { (e.target as HTMLSelectElement).style.borderColor = 'var(--accent)'; }}
      onBlur={(e) => { (e.target as HTMLSelectElement).style.borderColor = 'var(--border)'; }}
    >
      {options.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
    </select>
  );
}

function Toggle({ checked, onChange }: { checked: boolean | null; onChange: (v: boolean) => void }) {
  const on = checked ?? false;
  return (
    <label style={{ position: 'relative', display: 'inline-flex', cursor: 'pointer', flexShrink: 0 }}>
      <input
        type="checkbox"
        checked={on}
        onChange={(e) => onChange(e.target.checked)}
        style={{ position: 'absolute', opacity: 0, width: 0, height: 0 }}
      />
      <div
        style={{
          width: 34, height: 18, borderRadius: 9,
          background: on ? 'var(--accent)' : 'var(--bg-elevated)',
          border: `1px solid ${on ? 'var(--accent)' : 'var(--border)'}`,
          transition: 'background 150ms, border-color 150ms',
          position: 'relative',
        }}
      >
        <div
          style={{
            position: 'absolute', top: 2,
            left: on ? 16 : 2,
            width: 12, height: 12, borderRadius: 6,
            background: on ? '#fff' : 'var(--text-muted)',
            transition: 'left 150ms cubic-bezier(0.16, 1, 0.3, 1), background 150ms',
            boxShadow: on ? '0 1px 2px rgba(0,0,0,0.3)' : 'none',
          }}
        />
      </div>
    </label>
  );
}

function ToggleRow({ label, help, checked, onChange }: { label: string; help?: string; checked: boolean | null; onChange: (v: boolean) => void }) {
  return (
    <label style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12, cursor: 'pointer', padding: '2px 0' }}>
      <div>
        <div style={{ fontSize: 'var(--text-sm)', color: 'var(--text-primary)', fontWeight: 400 }}>{label}</div>
        {help && <div style={{ fontSize: '10px', color: 'var(--text-muted)', marginTop: 2, lineHeight: 1.4 }}>{help}</div>}
      </div>
      <Toggle checked={checked} onChange={onChange} />
    </label>
  );
}

/* ── Section separator ───────────────────────────────────────── */
function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <p style={{ fontSize: '11px', fontWeight: 500, color: 'var(--text-muted)', marginBottom: 12, marginTop: 4, userSelect: 'none' }}>
      {children}
    </p>
  );
}

function SectionDivider() {
  return <div style={{ height: 1, background: 'var(--border-subtle)', margin: '20px 0' }} />;
}

/* ── Tool-aware components ───────────────────────────────────── */
function ToolPriorityField({
  value, enabledTools, onChange, help,
}: { value: string[]; enabledTools: string[]; onChange: (v: string[]) => void; help?: string }) {
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
      <Field label="Tool Priority">
        <p style={{ fontSize: 'var(--text-sm)', color: 'var(--text-muted)' }}>No tools enabled.</p>
      </Field>
    );

  return (
    <div>
      <Label>Tool Priority</Label>
      <div style={{ border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', overflow: 'hidden' }}>
        {list.map((tool, idx) => {
          const isExplicit = value.includes(tool);
          return (
            <div
              key={tool}
              style={{
                display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                padding: '7px 10px',
                borderBottom: idx < list.length - 1 ? '1px solid var(--border-subtle)' : 'none',
                background: 'var(--bg-secondary)',
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <span style={{ fontSize: '10px', color: isExplicit ? 'var(--accent)' : 'var(--text-muted)', fontFamily: 'var(--font-mono)', width: 16, textAlign: 'right', flexShrink: 0 }}>
                  {idx + 1}.
                </span>
                <span style={{ fontSize: 'var(--text-sm)', fontFamily: 'var(--font-mono)', color: isExplicit ? 'var(--text-primary)' : 'var(--text-muted)' }}>
                  {tool}
                </span>
                {!isExplicit && <span style={{ fontSize: '10px', color: 'var(--text-muted)', fontStyle: 'italic' }}>default</span>}
              </div>
              <div style={{ display: 'flex', gap: 3 }}>
                <ArrowBtn onClick={() => move(idx, -1)} disabled={idx === 0} label="Move up">↑</ArrowBtn>
                <ArrowBtn onClick={() => move(idx, 1)} disabled={idx === list.length - 1} label="Move down">↓</ArrowBtn>
              </div>
            </div>
          );
        })}
      </div>
      {help && <Help>{help}</Help>}
    </div>
  );
}

function ArrowBtn({ children, onClick, disabled, label }: { children: React.ReactNode; onClick: () => void; disabled?: boolean; label: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={label}
      style={{
        width: 22, height: 22,
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        fontSize: 11,
        border: '1px solid var(--border)',
        borderRadius: 'var(--radius-sm)',
        background: 'var(--bg-card)',
        color: disabled ? 'var(--text-muted)' : 'var(--text-secondary)',
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.35 : 1,
        transition: 'background 80ms',
      }}
      onMouseEnter={(e) => { if (!disabled) (e.currentTarget as HTMLElement).style.background = 'var(--bg-elevated)'; }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--bg-card)'; }}
    >
      {children}
    </button>
  );
}

function MaxParallelPerToolField({
  value, enabledTools, onChange, help,
}: { value: Record<string, number>; enabledTools: string[]; onChange: (v: Record<string, number>) => void; help?: string }) {
  const adjust = (tool: string, delta: number) => {
    const current = value[tool] ?? 1;
    onChange({ ...value, [tool]: Math.max(1, current + delta) });
  };

  if (enabledTools.length === 0)
    return (
      <Field label="Max Parallel Per Tool">
        <p style={{ fontSize: 'var(--text-sm)', color: 'var(--text-muted)' }}>No tools enabled.</p>
      </Field>
    );

  return (
    <div>
      <Label>Max Parallel Per Tool</Label>
      <div style={{ border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', overflow: 'hidden' }}>
        {enabledTools.map((tool, idx) => {
          const count = value[tool] ?? 1;
          return (
            <div
              key={tool}
              style={{
                display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                padding: '7px 10px',
                borderBottom: idx < enabledTools.length - 1 ? '1px solid var(--border-subtle)' : 'none',
                background: 'var(--bg-secondary)',
              }}
            >
              <span style={{ fontSize: 'var(--text-sm)', fontFamily: 'var(--font-mono)', color: 'var(--text-primary)' }}>{tool}</span>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <ArrowBtn onClick={() => adjust(tool, -1)} disabled={count <= 1} label="Decrease">−</ArrowBtn>
                <span style={{ fontSize: 'var(--text-sm)', fontFamily: 'var(--font-mono)', color: 'var(--text-primary)', width: 20, textAlign: 'center', fontWeight: 500 }}>{count}</span>
                <ArrowBtn onClick={() => adjust(tool, 1)} label="Increase">+</ArrowBtn>
              </div>
            </div>
          );
        })}
      </div>
      {help && <Help>{help}</Help>}
    </div>
  );
}

function ToolSpecializationsField({
  value, enabledTools, onChange, help,
}: { value: Record<string, string[]>; enabledTools: string[]; onChange: (v: Record<string, string[]>) => void; help?: string }) {
  const [newCat, setNewCat] = useState('');

  const toggleTool = (cat: string, tool: string) => {
    const current = (value[cat] as string[]) ?? [];
    const next = current.includes(tool) ? current.filter((t) => t !== tool) : [...current, tool];
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
    <div>
      <Label>Tool Specializations</Label>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
        {Object.keys(value).length === 0 && (
          <p style={{ fontSize: 'var(--text-sm)', color: 'var(--text-muted)', padding: '6px 0' }}>
            No categories — all tools handle all task types.
          </p>
        )}
        {Object.entries(value).map(([cat, tools]) => (
          <div
            key={cat}
            style={{
              border: '1px solid var(--border)',
              borderRadius: 'var(--radius-md)',
              padding: '8px 10px',
              background: 'var(--bg-secondary)',
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
              <span style={{ fontSize: '11px', fontFamily: 'var(--font-mono)', fontWeight: 600, color: 'var(--text-primary)' }}>{cat}</span>
              <button
                type="button"
                onClick={() => removeCat(cat)}
                style={{ fontSize: '10px', color: 'var(--text-muted)', background: 'none', border: 'none', cursor: 'pointer', padding: '2px 4px' }}
                onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.color = 'var(--error)'; }}
                onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.color = 'var(--text-muted)'; }}
              >
                Remove
              </button>
            </div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 5 }}>
              {enabledTools.length === 0 && <span style={{ fontSize: '10px', color: 'var(--text-muted)' }}>No tools enabled.</span>}
              {enabledTools.map((tool) => {
                const active = (tools as string[]).includes(tool);
                return (
                  <button
                    key={tool}
                    type="button"
                    onClick={() => toggleTool(cat, tool)}
                    style={{
                      padding: '3px 9px',
                      fontSize: '11px',
                      fontFamily: 'var(--font-mono)',
                      borderRadius: 999,
                      border: `1px solid ${active ? 'var(--accent)' : 'var(--border)'}`,
                      background: active ? 'var(--accent-bg)' : 'var(--bg-card)',
                      color: active ? 'var(--accent)' : 'var(--text-muted)',
                      cursor: 'pointer',
                      transition: 'background 80ms, border-color 80ms, color 80ms',
                    }}
                  >
                    {tool}
                  </button>
                );
              })}
            </div>
          </div>
        ))}
        <div style={{ display: 'flex', gap: 6 }}>
          <input
            type="text"
            style={{ ...inputStyle, height: 30, flex: 1, fontSize: '12px' }}
            placeholder="New category (e.g. frontend)"
            value={newCat}
            onChange={(e) => setNewCat(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && addCat()}
            onFocus={(e) => { (e.target as HTMLInputElement).style.borderColor = 'var(--accent)'; }}
            onBlur={(e) => { (e.target as HTMLInputElement).style.borderColor = 'var(--border)'; }}
          />
          <button
            type="button"
            onClick={addCat}
            disabled={!newCat.trim()}
            style={{
              height: 30, padding: '0 12px',
              fontSize: '12px',
              border: '1px solid var(--border)',
              borderRadius: 'var(--radius-md)',
              background: 'var(--bg-card)',
              color: 'var(--text-secondary)',
              cursor: newCat.trim() ? 'pointer' : 'not-allowed',
              opacity: newCat.trim() ? 1 : 0.4,
              transition: 'background 80ms',
            }}
          >
            Add
          </button>
        </div>
      </div>
      {help && <Help>{help}</Help>}
    </div>
  );
}

/* ── Notice box ──────────────────────────────────────────────── */
function Notice({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        padding: '10px 12px',
        borderRadius: 'var(--radius-md)',
        background: 'oklch(0.75 0.17 80 / 0.08)',
        border: '1px solid oklch(0.75 0.17 80 / 0.25)',
        fontSize: '12px',
        color: 'var(--warning)',
        lineHeight: 1.5,
        marginBottom: 20,
      }}
    >
      {children}
    </div>
  );
}

/* ── Field grid layouts ──────────────────────────────────────── */
function FieldGrid({ cols = 2, children }: { cols?: number; children: React.ReactNode }) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: `repeat(${cols}, 1fr)`, gap: '14px 20px' }}>
      {children}
    </div>
  );
}

function FullWidth({ children }: { children: React.ReactNode }) {
  return <div style={{ gridColumn: '1 / -1' }}>{children}</div>;
}

/* ── Preset selector ─────────────────────────────────────────── */
function PresetBar({ onApply }: { onApply: (preset: 'conservative' | 'balanced' | 'throughput') => void }) {
  return (
    <div style={{ marginBottom: 20 }}>
      <SectionLabel>Presets</SectionLabel>
      <div style={{ display: 'flex', gap: 6 }}>
        {(['conservative', 'balanced', 'throughput'] as const).map((p) => (
          <button
            key={p}
            type="button"
            onClick={() => onApply(p)}
            style={{
              padding: '5px 12px',
              fontSize: '12px',
              fontWeight: 500,
              border: '1px solid var(--border)',
              borderRadius: 'var(--radius-md)',
              background: 'var(--bg-elevated)',
              color: 'var(--text-secondary)',
              cursor: 'pointer',
              transition: 'background 80ms, color 80ms',
              textTransform: 'capitalize',
            }}
            onMouseEnter={(e) => { const el = e.currentTarget; el.style.background = 'var(--accent-bg)'; el.style.color = 'var(--accent)'; el.style.borderColor = 'oklch(0.60 0.15 255 / 0.4)'; }}
            onMouseLeave={(e) => { const el = e.currentTarget; el.style.background = 'var(--bg-elevated)'; el.style.color = 'var(--text-secondary)'; el.style.borderColor = 'var(--border)'; }}
          >
            {p}
          </button>
        ))}
      </div>
    </div>
  );
}

/* ── Tab sections ────────────────────────────────────────────── */
function BasicTab({ draft, update }: { draft: ApiConfigResponse; update: (p: Partial<ApiConfigUpdateRequest>) => void }) {
  const handlePreset = (p: 'conservative' | 'balanced' | 'throughput') => {
    if (p === 'conservative') update({ maxParallel: 1, rateLimitFallbackEnabled: false, rateLimitThrottleParallel: false, mergeAiFix: false, safetyPolicy: 'strict', destructiveActions: 'double_confirm' });
    if (p === 'balanced') update({ maxParallel: 3, rateLimitFallbackEnabled: true, rateLimitThrottleParallel: false, mergeAiFix: true, safetyPolicy: 'standard', destructiveActions: 'double_confirm' });
    if (p === 'throughput') update({ maxParallel: 6, rateLimitFallbackEnabled: true, rateLimitThrottleParallel: true, mergeAiFix: true, safetyPolicy: 'standard', destructiveActions: 'double_confirm' });
  };

  return (
    <div>
      <PresetBar onApply={handlePreset} />

      <SectionLabel>General</SectionLabel>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 10, marginBottom: 4 }}>
        <ToggleRow label="Quiet mode" help="Suppress non-essential CLI output." checked={draft.quiet} onChange={(v) => update({ quiet: v })} />
        <ToggleRow label="Offline mode" help="Disable all remote network requests." checked={draft.offline} onChange={(v) => update({ offline: v })} />
        <ToggleRow label="Debug mode" help="Verbose performer logs — prompt dump, runner line, [MACC] invoke. Equivalent to MACC_DEBUG=1." checked={draft.debug} onChange={(v) => update({ debug: v })} />
      </div>

      <SectionDivider />
      <SectionLabel>Coordinator</SectionLabel>
      <FieldGrid cols={2}>
        <Field label="Coordinator tool" help="AI tool for coordinator phases (review, fix). Empty = auto-select.">
          <SelectInput
            value={draft.coordinatorTool}
            onChange={(v) => update({ coordinatorTool: v || null })}
            options={[
              { value: '', label: '— Auto-select —' },
              ...draft.enabledTools.map((t) => ({ value: t, label: t })),
            ]}
          />
        </Field>
        <Field label="Reference branch" help="Branch where completed worktrees are merged.">
          <TextInput value={draft.referenceBranch} onChange={(v) => update({ referenceBranch: v })} placeholder="main" />
        </Field>
        <Field label="Max parallel" help="Concurrent tasks the coordinator can run.">
          <NumberInput value={draft.maxParallel} onChange={(v) => update({ maxParallel: v })} placeholder="3" />
        </Field>
        <Field label="Timeout (s)" help="Global wall-clock timeout. 0 = unlimited.">
          <NumberInput value={draft.timeoutSeconds} onChange={(v) => update({ timeoutSeconds: v })} placeholder="0" />
        </Field>
        <Field label="Web port" help="Port the local dashboard server binds to.">
          <NumberInput value={draft.webPort} onChange={(v) => update({ webPort: v })} placeholder="3450" />
        </Field>
      </FieldGrid>

      <SectionDivider />
      <SectionLabel>Safety</SectionLabel>
      <FieldGrid cols={2}>
        <Field label="Safety policy" help="Verification applied to tool write operations.">
          <SelectInput
            value={draft.safetyPolicy ?? null}
            onChange={(v) => update({ safetyPolicy: v })}
            options={[
              { value: 'standard', label: 'Standard' },
              { value: 'strict', label: 'Strict' },
            ]}
          />
        </Field>
        <Field label="Destructive actions" help="Confirmation required for forced updates and checkouts.">
          <SelectInput
            value={draft.destructiveActions ?? null}
            onChange={(v) => update({ destructiveActions: v })}
            options={[
              { value: 'double_confirm', label: 'Double confirm' },
              { value: 'single_confirm', label: 'Single confirm' },
            ]}
          />
        </Field>
      </FieldGrid>
    </div>
  );
}

function AdvancedTab({ draft, update }: { draft: ApiConfigResponse; update: (p: Partial<ApiConfigUpdateRequest>) => void }) {
  return (
    <div>
      <Notice>
        Advanced settings control task scheduling, staleness thresholds, and retry behavior. Changes here can affect coordinator stability.
      </Notice>

      <SectionLabel>Routing</SectionLabel>
      <FieldGrid cols={2}>
        <FullWidth>
          <ToolPriorityField value={draft.toolPriority} enabledTools={draft.enabledTools} onChange={(v) => update({ toolPriority: v })} help="↑↓ buttons reorder priority. Tools not listed use default order." />
        </FullWidth>
        <FullWidth>
          <MaxParallelPerToolField value={draft.maxParallelPerTool as Record<string, number>} enabledTools={draft.enabledTools} onChange={(v) => update({ maxParallelPerTool: v })} help="Per-tool concurrency cap. Default is 1." />
        </FullWidth>
        <FullWidth>
          <ToolSpecializationsField value={draft.toolSpecializations as Record<string, string[]>} enabledTools={draft.enabledTools} onChange={(v) => update({ toolSpecializations: v })} help="Route task categories to specific tools. Empty = all tools handle all categories." />
        </FullWidth>
        <Field label="PRD file" help="Path to the task sequence definition file.">
          <TextInput value={draft.prdFile} onChange={(v) => update({ prdFile: v })} placeholder="prd.json" mono />
        </Field>
        <Field label="Max dispatch" help="Tasks dispatched per run. 0 = unlimited.">
          <NumberInput value={draft.maxDispatch} onChange={(v) => update({ maxDispatch: v })} placeholder="10" />
        </Field>
      </FieldGrid>

      <SectionDivider />
      <SectionLabel>Stale thresholds</SectionLabel>
      <FieldGrid cols={3}>
        <Field label="Stale claimed (s)" help="Auto-stale timeout for claimed tasks. 0 = disabled.">
          <NumberInput value={draft.staleClaimedSeconds} onChange={(v) => update({ staleClaimedSeconds: v })} />
        </Field>
        <Field label="In-progress timeout (s)" help="Hard kill timeout for performer processes. 0 = disabled.">
          <NumberInput value={draft.staleInProgressSeconds} onChange={(v) => update({ staleInProgressSeconds: v })} />
        </Field>
        <Field label="Changes-requested (s)" help="Auto-stale for changes-requested tasks. 0 = disabled.">
          <NumberInput value={draft.staleChangesRequestedSeconds} onChange={(v) => update({ staleChangesRequestedSeconds: v })} />
        </Field>
        <Field label="Stale action" help="Action on stale: block, retry, or requeue.">
          <SelectInput
            value={draft.staleAction ?? null}
            onChange={(v) => update({ staleAction: v })}
            options={[
              { value: 'block', label: 'Block' },
              { value: 'retry', label: 'Retry' },
              { value: 'requeue', label: 'Requeue' },
            ]}
          />
        </Field>
      </FieldGrid>

      <SectionDivider />
      <SectionLabel>Error retry</SectionLabel>
      <FieldGrid cols={2}>
        <Field label="Retry error codes" help="Comma-separated codes eligible for auto-retry.">
          <TextInput value={draft.errorCodeRetryList} onChange={(v) => update({ errorCodeRetryList: v })} placeholder="E601,E603" mono />
        </Field>
        <Field label="Max retries" help="Maximum retry attempts per task.">
          <NumberInput value={draft.errorCodeRetryMax} onChange={(v) => update({ errorCodeRetryMax: v })} placeholder="2" />
        </Field>
      </FieldGrid>

      <SectionDivider />
      <SectionLabel>Rate limiting</SectionLabel>
      <FieldGrid cols={2}>
        <Field label="Backoff base (s)" help="Initial delay on first E601 rate-limit.">
          <NumberInput value={draft.rateLimitBackoffBaseSeconds} onChange={(v) => update({ rateLimitBackoffBaseSeconds: v })} placeholder="30" />
        </Field>
        <Field label="Backoff max (s)" help="Cap for exponential backoff growth.">
          <NumberInput value={draft.rateLimitBackoffMaxSeconds} onChange={(v) => update({ rateLimitBackoffMaxSeconds: v })} placeholder="300" />
        </Field>
        <FullWidth>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            <ToggleRow label="Rate-limit fallback" help="On throttle, fall back to the next available tool." checked={draft.rateLimitFallbackEnabled} onChange={(v) => update({ rateLimitFallbackEnabled: v })} />
            <ToggleRow label="Throttle parallel" help="Reduce concurrency automatically on rate-limit." checked={draft.rateLimitThrottleParallel} onChange={(v) => update({ rateLimitThrottleParallel: v })} />
          </div>
        </FullWidth>
      </FieldGrid>

      <SectionDivider />
      <SectionLabel>Merge behavior</SectionLabel>
      <FieldGrid cols={2}>
        <ToggleRow label="AI merge fix" help="Enable AI-driven resolution for merge conflicts." checked={draft.mergeAiFix} onChange={(v) => update({ mergeAiFix: v })} />
        <Field label="Merge job timeout (s)" help="Timeout for git merge operations.">
          <NumberInput value={draft.mergeJobTimeoutSeconds} onChange={(v) => update({ mergeJobTimeoutSeconds: v })} />
        </Field>
        <Field label="Merge hook timeout (s)" help="Timeout for AI merge-fix hook execution." >
          <NumberInput value={draft.mergeHookTimeoutSeconds} onChange={(v) => update({ mergeHookTimeoutSeconds: v })} placeholder="90" />
        </Field>
      </FieldGrid>

      <SectionDivider />
      <SectionLabel>Lifecycle</SectionLabel>
      <FieldGrid cols={3}>
        <Field label="Phase runner attempts" help="Max attempts for phase runner fallback.">
          <NumberInput value={draft.phaseRunnerMaxAttempts} onChange={(v) => update({ phaseRunnerMaxAttempts: v })} placeholder="1" />
        </Field>
        <Field label="Dispatch cooldown (s)" help="Wait between dispatch cycles.">
          <NumberInput value={draft.dispatchCooldownSeconds} onChange={(v) => update({ dispatchCooldownSeconds: v })} placeholder="2" />
        </Field>
        <Field label="Force-kill grace (s)" help="Wait after IPC failure before force-killing.">
          <NumberInput value={draft.forceKillGraceSeconds} onChange={(v) => update({ forceKillGraceSeconds: v })} />
        </Field>
        <Field label="Ghost heartbeat grace (s)" help="Grace before treating a dead process as a ghost.">
          <NumberInput value={draft.ghostHeartbeatGraceSeconds} onChange={(v) => update({ ghostHeartbeatGraceSeconds: v })} placeholder="30" />
        </Field>
        <Field label="Max review cycles" help="Max review loops per task. 0 = skip review.">
          <NumberInput value={draft.maxReviewCycles ?? null} onChange={(v) => update({ maxReviewCycles: v })} />
        </Field>
      </FieldGrid>
    </div>
  );
}

function AdminTab({ draft, update }: { draft: ApiConfigResponse; update: (p: Partial<ApiConfigUpdateRequest>) => void }) {
  return (
    <div>
      <Notice>
        Admin settings control storage engines, migration layers, and internal runtime gates. For system administrators and debugging only.
      </Notice>

      <SectionLabel>Storage</SectionLabel>
      <FieldGrid cols={2}>
        <Field label="Storage mode" help="Coordinator storage engine: json or sqlite.">
          <SelectInput
            value={draft.storageMode ?? null}
            onChange={(v) => update({ storageMode: v })}
            options={[
              { value: 'json', label: 'JSON' },
              { value: 'sqlite', label: 'SQLite' },
              { value: 'dual-write', label: 'Dual-write' },
            ]}
          />
        </Field>
        <Field label="Task registry file" help="File path for local task registry state.">
          <TextInput value={draft.taskRegistryFile} onChange={(v) => update({ taskRegistryFile: v })} placeholder=".macc/automation/task/task_registry.json" mono />
        </Field>
      </FieldGrid>

      <SectionDivider />
      <SectionLabel>Log flushing</SectionLabel>
      <FieldGrid cols={3}>
        <Field label="Flush every N lines" help="0 uses runtime default.">
          <NumberInput value={draft.logFlushLines} onChange={(v) => update({ logFlushLines: v })} />
        </Field>
        <Field label="Flush every N ms" help="0 uses runtime default.">
          <NumberInput value={draft.logFlushMs} onChange={(v) => update({ logFlushMs: v })} />
        </Field>
        <Field label="Mirror JSON debounce (ms)" help="Debounce SQLite-to-JSON compatibility export.">
          <NumberInput value={draft.mirrorJsonDebounceMs} onChange={(v) => update({ mirrorJsonDebounceMs: v })} />
        </Field>
      </FieldGrid>

      <SectionDivider />
      <SectionLabel>Compatibility</SectionLabel>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <ToggleRow label="JSON compatibility" help="Enable JSON snapshot export for external tool compatibility." checked={draft.jsonCompat} onChange={(v) => update({ jsonCompat: v })} />
        <ToggleRow label="Legacy JSON fallback" help="Fall back to JSON registry if SQLite is corrupted." checked={draft.legacyJsonFallback} onChange={(v) => update({ legacyJsonFallback: v })} />
      </div>

      <SectionDivider />
      <SectionLabel>Cutover gate</SectionLabel>
      <FieldGrid cols={3}>
        <Field label="Window events" help="Recent events inspected to assess storage health.">
          <NumberInput value={draft.cutoverGateWindowEvents} onChange={(v) => update({ cutoverGateWindowEvents: v })} placeholder="2000" />
        </Field>
        <Field label="Max blocked ratio" help="Maximum ratio of blocked events before cutover block.">
          <NumberInput value={draft.cutoverGateMaxBlockedRatio} onChange={(v) => update({ cutoverGateMaxBlockedRatio: v })} placeholder="0.25" />
        </Field>
        <Field label="Max stale ratio" help="Maximum ratio of stale events before cutover block.">
          <NumberInput value={draft.cutoverGateMaxStaleRatio} onChange={(v) => update({ cutoverGateMaxStaleRatio: v })} placeholder="0.25" />
        </Field>
      </FieldGrid>
    </div>
  );
}

function RawTab({ config, onApplyRaw }: { config: ApiConfigResponse; onApplyRaw: (raw: string) => void }) {
  const [rawText, setRawText] = useState(() => JSON.stringify(config, null, 2));
  const [parseError, setParseError] = useState<string | null>(null);
  const [prevConfig, setPrevConfig] = useState(config);

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
    <div>
      <SectionLabel>Raw JSON</SectionLabel>
      <p style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: 12, lineHeight: 1.5 }}>
        Edit the full configuration as JSON. Changes apply when you click "Apply JSON" and then save.
      </p>
      {parseError && <ErrorBanner message={parseError} />}
      <textarea
        style={{
          width: '100%', minHeight: 400,
          borderRadius: 'var(--radius-md)',
          border: `1px solid ${parseError ? 'var(--error)' : 'var(--border)'}`,
          background: 'var(--bg-secondary)',
          color: 'var(--text-primary)',
          padding: 14,
          fontFamily: 'var(--font-mono)',
          fontSize: '11px',
          lineHeight: 1.6,
          outline: 'none',
          resize: 'vertical',
          boxSizing: 'border-box',
          transition: 'border-color 100ms',
        }}
        value={rawText}
        onChange={(e) => { setRawText(e.target.value); setParseError(null); }}
        spellCheck={false}
        onFocus={(e) => { (e.target as HTMLTextAreaElement).style.borderColor = parseError ? 'var(--error)' : 'var(--accent)'; }}
        onBlur={(e) => { (e.target as HTMLTextAreaElement).style.borderColor = parseError ? 'var(--error)' : 'var(--border)'; }}
      />
      <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 10 }}>
        <SaveBtn onClick={handleApply} label="Apply JSON" />
      </div>
    </div>
  );
}

/* ── Save / discard buttons ──────────────────────────────────── */
function SaveBtn({ onClick, label, disabled, loading }: { onClick: () => void; label?: string; disabled?: boolean; loading?: boolean }) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      style={{
        height: 32,
        padding: '0 16px',
        borderRadius: 'var(--radius-md)',
        border: 'none',
        background: disabled ? 'var(--accent-bg)' : 'var(--accent)',
        color: disabled ? 'var(--accent-muted)' : '#fff',
        fontSize: 'var(--text-sm)',
        fontWeight: 500,
        cursor: disabled ? 'not-allowed' : 'pointer',
        transition: 'background 100ms, opacity 100ms',
        opacity: disabled ? 0.5 : 1,
      }}
      onMouseEnter={(e) => { if (!disabled) (e.currentTarget as HTMLElement).style.background = 'var(--accent-hover)'; }}
      onMouseLeave={(e) => { if (!disabled) (e.currentTarget as HTMLElement).style.background = 'var(--accent)'; }}
    >
      {loading ? 'Saving…' : (label ?? 'Save changes')}
    </button>
  );
}

function DiscardBtn({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        height: 32, padding: '0 12px',
        borderRadius: 'var(--radius-md)',
        border: '1px solid var(--border)',
        background: 'none',
        color: 'var(--text-muted)',
        fontSize: 'var(--text-sm)',
        cursor: 'pointer',
        transition: 'color 100ms, border-color 100ms',
      }}
      onMouseEnter={(e) => { const el = e.currentTarget; el.style.color = 'var(--text-primary)'; el.style.borderColor = 'var(--text-muted)'; }}
      onMouseLeave={(e) => { const el = e.currentTarget; el.style.color = 'var(--text-muted)'; el.style.borderColor = 'var(--border)'; }}
    >
      Discard
    </button>
  );
}

/* ── Navigation items ────────────────────────────────────────── */
const NAV: { key: SettingsSection; label: string; sub: string }[] = [
  { key: 'basic',    label: 'Basic',    sub: 'Coordinator & general' },
  { key: 'advanced', label: 'Advanced', sub: 'Scheduling & routing' },
  { key: 'admin',    label: 'Admin',    sub: 'Storage & compat.' },
  { key: 'raw',      label: 'Raw JSON', sub: 'Direct editing' },
];

const BASIC_KEYS = ['webPort','offline','quiet','debug','coordinatorTool','maxParallel','timeoutSeconds','safetyPolicy','destructiveActions','referenceBranch'];
const ADVANCED_KEYS = ['prdFile','toolPriority','maxParallelPerTool','toolSpecializations','maxDispatch','phaseRunnerMaxAttempts','dispatchCooldownSeconds','staleClaimedSeconds','staleInProgressSeconds','staleChangesRequestedSeconds','staleAction','mergeAiFix','mergeJobTimeoutSeconds','mergeHookTimeoutSeconds','ghostHeartbeatGraceSeconds','errorCodeRetryList','errorCodeRetryMax','rateLimitBackoffBaseSeconds','rateLimitBackoffMaxSeconds','rateLimitFallbackEnabled','rateLimitThrottleParallel','forceKillGraceSeconds','maxReviewCycles'];
const ADMIN_KEYS = ['storageMode','taskRegistryFile','logFlushLines','logFlushMs','mirrorJsonDebounceMs','jsonCompat','legacyJsonFallback','cutoverGateWindowEvents','cutoverGateMaxBlockedRatio','cutoverGateMaxStaleRatio'];

function sectionForKey(key: string): SettingsSection {
  if (BASIC_KEYS.some((k) => key.startsWith(k))) return 'basic';
  if (ADVANCED_KEYS.some((k) => key.startsWith(k))) return 'advanced';
  if (ADMIN_KEYS.some((k) => key.startsWith(k))) return 'admin';
  return 'raw';
}

/* ── Main Settings page ──────────────────────────────────────── */
const Settings: React.FC = () => {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [config, setConfig] = useState<ApiConfigResponse | null>(null);
  const [draft, setDraft] = useState<ApiConfigResponse | null>(null);
  const [activeSection, setActiveSection] = useState<SettingsSection>('basic');
  const [toast, setToast] = useState<ToastState>({ open: false, title: '', variant: 'success' });
  const location = useLocation();

  const isDirty = config !== null && draft !== null && JSON.stringify(config) !== JSON.stringify(draft);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    getConfig()
      .then((res) => {
        if (cancelled) return;
        const n = normalizeConfigResponse(res);
        setConfig(n);
        setDraft(n);
      })
      .catch((err) => { if (!cancelled) setError(formatError(err)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    const state = location.state as { highlightSettingKey?: string } | null;
    if (state?.highlightSettingKey) {
      setActiveSection(sectionForKey(state.highlightSettingKey));
    }
  }, [location.state]);

  const updateDraft = useCallback((patch: Partial<ApiConfigUpdateRequest>) => {
    setDraft((prev) => (prev ? { ...prev, ...patch } : prev));
  }, []);

  const handleDiscard = useCallback(() => { setDraft(config); }, [config]);

  const handleSave = useCallback(async () => {
    if (!draft) return;
    setSaving(true);
    try {
      const updated = normalizeConfigResponse(await updateConfig(draft as ApiConfigUpdateRequest));
      setConfig(updated);
      setDraft(updated);
      setToast({ open: true, title: 'Settings saved', variant: 'success' });
    } catch (err) {
      setToast({ open: true, title: 'Failed to save', description: formatError(err), variant: 'error' });
    } finally {
      setSaving(false);
    }
  }, [draft]);

  const handleApplyRaw = useCallback((raw: string) => {
    try {
      const parsed = normalizeConfigResponse(JSON.parse(raw) as ApiConfigResponse);
      setDraft(parsed);
    } catch { /* handled in RawTab */ }
  }, []);

  if (loading) return <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', padding: '60px 0' }}><LoadingSpinner label="Loading settings…" /></div>;
  if (error || !draft || !config) return <div style={{ padding: '24px 0' }}><ErrorBanner message={error ?? 'Failed to load configuration.'} /></div>;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 0 }}>
      {/* Page header */}
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 20 }}>
        <div>
          <h1 style={{ fontSize: '16px', fontWeight: 600, color: 'var(--text-primary)', letterSpacing: '-0.01em', margin: 0 }}>Settings</h1>
          {isDirty && (
            <p style={{ fontSize: '11px', color: 'var(--warning)', marginTop: 3 }}>Unsaved changes</p>
          )}
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          {isDirty && <DiscardBtn onClick={handleDiscard} />}
          <SaveBtn onClick={handleSave} disabled={!isDirty || saving} loading={saving} />
        </div>
      </div>

      {/* Two-column layout */}
      <div style={{ display: 'grid', gridTemplateColumns: '156px 1fr', gap: 24, alignItems: 'start' }}>
        {/* Sidebar nav */}
        <nav style={{ position: 'sticky', top: 0 }}>
          {NAV.map((item) => {
            const active = activeSection === item.key;
            return (
              <button
                key={item.key}
                type="button"
                onClick={() => setActiveSection(item.key)}
                style={{
                  display: 'block',
                  width: '100%',
                  textAlign: 'left',
                  padding: '8px 10px',
                  borderRadius: 'var(--radius-md)',
                  border: 'none',
                  background: active ? 'var(--accent-bg)' : 'none',
                  cursor: 'pointer',
                  transition: 'background 100ms',
                  marginBottom: 2,
                }}
                onMouseEnter={(e) => { if (!active) (e.currentTarget as HTMLElement).style.background = 'var(--bg-elevated)'; }}
                onMouseLeave={(e) => { if (!active) (e.currentTarget as HTMLElement).style.background = 'none'; }}
              >
                <div style={{ fontSize: 'var(--text-sm)', fontWeight: active ? 500 : 400, color: active ? 'var(--accent)' : 'var(--text-secondary)' }}>
                  {item.label}
                </div>
                <div style={{ fontSize: '10px', color: active ? 'var(--accent-muted)' : 'var(--text-muted)', marginTop: 1 }}>
                  {item.sub}
                </div>
              </button>
            );
          })}
        </nav>

        {/* Content panel */}
        <div
          style={{
            background: 'var(--bg-card)',
            border: '1px solid var(--border)',
            borderRadius: 'var(--radius-lg)',
            padding: '20px 24px',
            minWidth: 0,
          }}
        >
          {activeSection === 'basic'    && <BasicTab    draft={draft} update={updateDraft} />}
          {activeSection === 'advanced' && <AdvancedTab draft={draft} update={updateDraft} />}
          {activeSection === 'admin'    && <AdminTab    draft={draft} update={updateDraft} />}
          {activeSection === 'raw'      && <RawTab config={draft} onApplyRaw={handleApplyRaw} />}
        </div>
      </div>

      <Toast
        open={toast.open}
        onOpenChange={(open) => setToast((p) => ({ ...p, open }))}
        title={toast.title}
        description={toast.description}
        variant={toast.variant}
      />
    </div>
  );
};

export default Settings;
