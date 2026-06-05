import React from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import { createWorktree, getConfig, getGitGraph } from '../api/client';
import type { ApiWorktree } from '../api/models';
import { Button } from './Button';
import { LoadingSpinner } from './LoadingSpinner';
import * as Icons from './icons';

type WizardStep = 0 | 1 | 2 | 3;

const STEPS = ['Basics', 'Branch & Scope', 'Review', 'Create'] as const;

interface WorktreeWizardProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onComplete?: () => Promise<void> | void;
  worktrees: ApiWorktree[];
}

interface WizardFormState {
  slug: string;
  tool: string;
  count: number;
  base: string;
  scopeCsv: string;
  autoApply: boolean;
}

const DEFAULT_FORM_STATE: WizardFormState = {
  slug: '',
  tool: '',
  count: 1,
  base: 'main',
  scopeCsv: '',
  autoApply: false,
};

function parseScopeCsv(value: string): string | null {
  const values = value
    .split(',')
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
  if (values.length === 0) {
    return null;
  }
  return values.join(', ');
}

export const WorktreeWizard: React.FC<WorktreeWizardProps> = ({
  open,
  onOpenChange,
  onComplete,
  worktrees,
}) => {
  const [step, setStep] = React.useState<WizardStep>(0);
  const [form, setForm] = React.useState<WizardFormState>(DEFAULT_FORM_STATE);
  const [tools, setTools] = React.useState<string[]>([]);
  const [baseBranchSuggestions, setBaseBranchSuggestions] = React.useState<string[]>(['main']);
  const [isBootstrapping, setIsBootstrapping] = React.useState(false);
  const [isCreating, setIsCreating] = React.useState(false);
  const [validationError, setValidationError] = React.useState<string | null>(null);
  const [createError, setCreateError] = React.useState<string | null>(null);
  const [created, setCreated] = React.useState<ApiWorktree[] | null>(null);

  React.useEffect(() => {
    if (!open) {
      return;
    }

    let cancelled = false;
    setIsBootstrapping(true);
    setStep(0);
    setForm(DEFAULT_FORM_STATE);
    setValidationError(null);
    setCreateError(null);
    setCreated(null);

    Promise.all([getConfig(), getGitGraph({ limit: 120 })])
      .then(([config, graph]) => {
        if (cancelled) {
          return;
        }

        const enabledTools = config.enabledTools.filter((entry) => entry.trim().length > 0);
        const selectedTool = enabledTools[0] ?? 'codex';
        const mergedBranches = Array.from(new Set([graph.head, ...graph.branches, 'main', 'master'].filter(Boolean)));
        setTools(enabledTools.length > 0 ? enabledTools : ['codex']);
        setBaseBranchSuggestions(mergedBranches);
        setForm((prev) => ({
          ...prev,
          tool: selectedTool,
          base: mergedBranches[0] ?? 'main',
        }));
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }
        const message = error instanceof Error ? error.message : 'Failed to load wizard defaults.';
        setValidationError(message);
        setTools(['codex']);
      })
      .finally(() => {
        if (!cancelled) {
          setIsBootstrapping(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [open]);

  const setField = React.useCallback(<K extends keyof WizardFormState>(key: K, value: WizardFormState[K]) => {
    setForm((prev) => ({ ...prev, [key]: value }));
    setValidationError(null);
  }, []);

  const idPreview = React.useMemo(() => {
    if (form.count === 1) {
      return [`${form.slug || '<slug>'}`];
    }
    return Array.from({ length: form.count }, (_, index) => `${form.slug || '<slug>'}-${String(index + 1).padStart(2, '0')}`);
  }, [form.count, form.slug]);

  const inferredRoot = React.useMemo(() => {
    if (worktrees.length === 0) {
      return '.macc/worktree';
    }
    const withNestedPath = worktrees.find((entry) => entry.path.includes('/.macc/worktree/'));
    if (withNestedPath) {
      return withNestedPath.path.split('/.macc/worktree/')[0] + '/.macc/worktree';
    }
    return '.macc/worktree';
  }, [worktrees]);

  const scopeValue = React.useMemo(() => parseScopeCsv(form.scopeCsv), [form.scopeCsv]);

  const validateStep = React.useCallback((candidateStep: WizardStep): string | null => {
    if (candidateStep === 0) {
      if (form.slug.trim().length === 0) {
        return 'Slug is required.';
      }
      if (form.tool.trim().length === 0) {
        return 'Tool selection is required.';
      }
      if (!Number.isInteger(form.count) || form.count < 1) {
        return 'Count must be at least 1.';
      }
    }
    if (candidateStep === 1 && form.base.trim().length === 0) {
      return 'Base branch is required.';
    }
    return null;
  }, [form.base, form.count, form.slug, form.tool]);

  const handleNext = React.useCallback(() => {
    const issue = validateStep(step);
    if (issue) {
      setValidationError(issue);
      return;
    }
    setValidationError(null);
    setStep((prev) => Math.min(prev + 1, 3) as WizardStep);
  }, [step, validateStep]);

  const handleBack = React.useCallback(() => {
    setValidationError(null);
    setStep((prev) => Math.max(prev - 1, 0) as WizardStep);
  }, []);

  const handleCreate = React.useCallback(async () => {
    const basicsIssue = validateStep(0);
    if (basicsIssue) {
      setValidationError(basicsIssue);
      setStep(0);
      return;
    }

    const branchIssue = validateStep(1);
    if (branchIssue) {
      setValidationError(branchIssue);
      setStep(1);
      return;
    }

    setStep(3);
    setValidationError(null);
    setCreateError(null);
    setIsCreating(true);

    try {
      const response = await createWorktree({
        slug: form.slug.trim(),
        tool: form.tool.trim(),
        count: form.count,
        base: form.base.trim(),
        scope: scopeValue,
        skipApply: !form.autoApply,
        allowUserScope: true,
      });
      setCreated(response);
    } catch (error) {
      setCreateError(error instanceof Error ? error.message : 'Failed to create worktree.');
    } finally {
      setIsCreating(false);
    }
  }, [form.autoApply, form.base, form.count, form.slug, form.tool, scopeValue, validateStep]);

  const handleDone = React.useCallback(async () => {
    await onComplete?.();
    onOpenChange(false);
  }, [onComplete, onOpenChange]);

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/70 backdrop-blur-[1px]" />
        <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-[min(96vw,56rem)] -translate-x-1/2 -translate-y-1/2 rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] p-6 text-[var(--text-primary)] shadow-2xl focus:outline-none">
          <Dialog.Title className="text-xl font-semibold">Create Worktree Wizard</Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-[var(--text-secondary)]">
            Configure basics, branch and scope, then review and create.
          </Dialog.Description>

          <ol className="mt-5 grid grid-cols-4 gap-2" aria-label="Wizard progress">
            {STEPS.map((entry, index) => {
              const isDone = index < step;
              const isActive = index === step;
              return (
                <li key={entry} className={`rounded-lg border px-3 py-2 text-xs ${isActive ? 'border-[var(--accent)] bg-[var(--accent)]/10 text-[var(--text-primary)]' : isDone ? 'border-emerald-500/40 bg-emerald-500/10 text-emerald-300' : 'border-[var(--border)] text-[var(--text-muted)]'}`}>
                  <span className="block text-[10px] uppercase tracking-wide">Step {index + 1}</span>
                  <span className="font-medium">{entry}</span>
                </li>
              );
            })}
          </ol>

          <div className="mt-5 min-h-[280px] rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4">
            {isBootstrapping ? (
              <div className="flex h-[240px] items-center justify-center gap-3 text-sm text-[var(--text-secondary)]">
                <LoadingSpinner size="md" />
                Loading defaults...
              </div>
            ) : step === 0 ? (
              <div className="grid gap-4 md:grid-cols-2">
                <label className="flex flex-col gap-1 text-sm">
                  <span className="text-[var(--text-secondary)]">Slug *</span>
                  <input value={form.slug} onChange={(event) => setField('slug', event.target.value)} placeholder="feature-api-hardening" className="rounded-lg border border-[var(--border)] bg-[var(--bg-card)] px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40" />
                </label>

                <label className="flex flex-col gap-1 text-sm">
                  <span className="text-[var(--text-secondary)]">Tool *</span>
                  <select value={form.tool} onChange={(event) => setField('tool', event.target.value)} className="rounded-lg border border-[var(--border)] bg-[var(--bg-card)] px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40">
                    {tools.map((tool) => (
                      <option key={tool} value={tool}>{tool}</option>
                    ))}
                  </select>
                </label>

                <label className="flex flex-col gap-1 text-sm md:col-span-2">
                  <span className="text-[var(--text-secondary)]">Count *</span>
                  <input type="number" min={1} max={20} value={form.count} onChange={(event) => setField('count', Math.max(1, Number.parseInt(event.target.value, 10) || 1))} className="rounded-lg border border-[var(--border)] bg-[var(--bg-card)] px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40" />
                </label>
              </div>
            ) : step === 1 ? (
              <div className="grid gap-4">
                <label className="flex flex-col gap-1 text-sm">
                  <span className="text-[var(--text-secondary)]">Base branch *</span>
                  <input value={form.base} onChange={(event) => setField('base', event.target.value)} list="base-branch-suggestions" className="rounded-lg border border-[var(--border)] bg-[var(--bg-card)] px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40" />
                  <datalist id="base-branch-suggestions">
                    {baseBranchSuggestions.map((branch) => (
                      <option key={branch} value={branch} />
                    ))}
                  </datalist>
                </label>

                <label className="flex flex-col gap-1 text-sm">
                  <span className="text-[var(--text-secondary)]">Scope (CSV, optional)</span>
                  <input value={form.scopeCsv} onChange={(event) => setField('scopeCsv', event.target.value)} placeholder="web/src/components,web/src/pages" className="rounded-lg border border-[var(--border)] bg-[var(--bg-card)] px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/40" />
                </label>
              </div>
            ) : step === 2 ? (
              <div className="space-y-4 text-sm">
                <div className="rounded-lg border border-[var(--border)] bg-[var(--bg-card)] p-3">
                  <p className="font-semibold text-[var(--text-primary)]">Summary</p>
                  <p className="mt-1 text-[var(--text-secondary)]">Creating {form.count} worktree{form.count > 1 ? 's' : ''} from <code>{form.base}</code> using tool <code>{form.tool}</code>.</p>
                  {scopeValue ? <p className="mt-1 text-[var(--text-secondary)]">Scope: <code>{scopeValue}</code></p> : null}
                </div>

                <div className="rounded-lg border border-[var(--border)] bg-[var(--bg-card)] p-3">
                  <p className="font-semibold">Paths and branches to be created</p>
                  <ul className="mt-2 space-y-1 font-mono text-xs text-[var(--text-secondary)]">
                    {idPreview.map((id) => (
                      <li key={id}>
                        <span>{inferredRoot}/{id}</span>
                        <span>{' -> '}</span>
                        <span>ai/{form.tool || '<tool>'}/{id}</span>
                      </li>
                    ))}
                  </ul>
                </div>

                <div className="rounded-lg border border-[var(--border)] bg-[var(--bg-card)] p-3">
                  <p className="font-semibold">Files expected to be written per worktree</p>
                  <ul className="mt-2 list-disc space-y-1 pl-5 text-[var(--text-secondary)]">
                    <li><code>.macc/worktree.json</code></li>
                    <li><code>.macc/tool.json</code></li>
                    <li><code>.macc/macc.yaml</code></li>
                    {scopeValue ? <li><code>.macc/scope.md</code></li> : null}
                  </ul>
                </div>

                <label className="flex items-center gap-2 text-[var(--text-secondary)]">
                  <input type="checkbox" checked={form.autoApply} onChange={(event) => setField('autoApply', event.target.checked)} />
                  Run apply automatically after creation
                </label>
              </div>
            ) : (
              <div className="space-y-4 text-sm">
                {isCreating ? (
                  <div className="flex items-center gap-3 rounded-lg border border-[var(--border)] bg-[var(--bg-card)] p-4 text-[var(--text-secondary)]">
                    <LoadingSpinner size="md" />
                    Creating worktree{form.count > 1 ? 's' : ''}...
                  </div>
                ) : created ? (
                  <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-4">
                    <p className="flex items-center gap-2 font-semibold text-emerald-300">
                      <Icons.CheckCircleIcon className="h-4 w-4" />
                      Worktree creation complete
                    </p>
                    <ul className="mt-2 space-y-1 text-xs text-emerald-100">
                      {created.map((entry) => (
                        <li key={entry.id}>
                          {entry.id} {'->'} {entry.path} ({entry.branch ?? 'no branch'})
                        </li>
                      ))}
                    </ul>
                  </div>
                ) : createError ? (
                  <div className="rounded-lg border border-rose-500/40 bg-rose-500/10 p-4 text-rose-200">
                    <p className="font-semibold">Worktree creation failed</p>
                    <p className="mt-1 text-xs">{createError}</p>
                  </div>
                ) : (
                  <div className="rounded-lg border border-[var(--border)] bg-[var(--bg-card)] p-4 text-[var(--text-secondary)]">
                    Ready to create. Click Create from the previous step.
                  </div>
                )}
              </div>
            )}
          </div>

          {validationError ? (
            <p className="mt-3 text-sm text-rose-300">{validationError}</p>
          ) : null}

          <div className="mt-5 flex items-center justify-between">
            <Button onClick={() => onOpenChange(false)} disabled={isCreating} className="bg-transparent hover:bg-white/10">
              Cancel
            </Button>
            <div className="flex items-center gap-2">
              <Button onClick={handleBack} disabled={isBootstrapping || isCreating || step === 0} className="bg-transparent hover:bg-white/10">
                Back
              </Button>

              {step < 2 ? (
                <Button onClick={handleNext} disabled={isBootstrapping || isCreating}>Next</Button>
              ) : step === 2 ? (
                <Button onClick={handleCreate} disabled={isBootstrapping || isCreating}>Create</Button>
              ) : (
                <Button onClick={handleDone} disabled={isBootstrapping || isCreating || !created}>Done</Button>
              )}
            </div>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
};
