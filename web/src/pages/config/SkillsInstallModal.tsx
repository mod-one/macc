import React from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import { Button } from '../../components';
import {
  buildManifestFromDraft,
  deriveRiskTags,
  hasPostInstallScripts,
  kindLabel,
  type CatalogKind,
  type InstallDraft,
  type SourceKind,
} from './skillsCatalog';

type ReviewTab = 'security' | 'configuration' | 'manifest';

interface SkillsInstallModalProps {
  draft: InstallDraft | null;
  isSaving: boolean;
  onClose: () => void;
  onInstall: (manifestText: string, draft: InstallDraft) => void;
}

export default function SkillsInstallModal({
  draft,
  isSaving,
  onClose,
  onInstall,
}: SkillsInstallModalProps) {
  const [reviewTab, setReviewTab] = React.useState<ReviewTab>('security');
  const [localDraft, setLocalDraft] = React.useState<InstallDraft | null>(null);
  const [manifestText, setManifestText] = React.useState('');
  const [manifestError, setManifestError] = React.useState<string | null>(null);

  React.useEffect(() => {
    setLocalDraft(draft);
    if (draft) {
      setManifestText(JSON.stringify(buildManifestFromDraft(draft), null, 2));
    } else {
      setManifestText('');
    }
    setManifestError(null);
    setReviewTab('security');
  }, [draft]);

  const parsedManifest = React.useMemo(() => {
    try {
      const parsed = JSON.parse(manifestText) as unknown;
      if (typeof parsed === 'object' && parsed !== null && !Array.isArray(parsed)) {
        return parsed as Record<string, unknown>;
      }
      return null;
    } catch {
      return null;
    }
  }, [manifestText]);

  const postInstallBlocked = parsedManifest ? hasPostInstallScripts(parsedManifest) : false;

  const updateDraft = React.useCallback((mutate: (current: InstallDraft) => InstallDraft) => {
    setLocalDraft((current) => {
      if (!current) {
        return current;
      }
      const next = mutate(current);
      setManifestText(JSON.stringify(buildManifestFromDraft(next), null, 2));
      setManifestError(null);
      return next;
    });
  }, []);

  return (
    <Dialog.Root open={Boolean(localDraft)} onOpenChange={(open) => (!open ? onClose() : null)}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/70" />
        <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-[min(96vw,58rem)] -translate-x-1/2 -translate-y-1/2 rounded-2xl border border-slate-300 bg-white p-5 shadow-2xl focus:outline-none">
          <div className="flex items-start justify-between gap-3">
            <div>
              <Dialog.Title className="text-xl font-semibold text-slate-950">Security Review</Dialog.Title>
              <Dialog.Description className="text-sm text-slate-600">
                Review risks and manifest before installing.
              </Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <button className="rounded-md border border-slate-300 px-2 py-1 text-sm text-slate-700" type="button">
                Close
              </button>
            </Dialog.Close>
          </div>

          {localDraft && (
            <div className="mt-4 flex flex-col gap-4">
              <div className="flex gap-2">
                {(['security', 'configuration', 'manifest'] as const).map((tab) => (
                  <button
                    key={tab}
                    className={`rounded-md px-3 py-1.5 text-sm ${reviewTab === tab ? 'bg-slate-900 text-white' : 'bg-slate-100 text-slate-700'}`}
                    onClick={() => setReviewTab(tab)}
                    type="button"
                  >
                    {tab === 'security' ? 'Security Review' : tab === 'configuration' ? 'Configuration' : 'Manifest'}
                  </button>
                ))}
              </div>

              {reviewTab === 'security' && (
                <div className="rounded-xl border border-slate-200 p-4">
                  <div className="grid grid-cols-1 gap-2 md:grid-cols-2 text-sm text-slate-700">
                    <div>
                      <div><span className="font-medium">Package:</span> {localDraft.name}</div>
                      <div><span className="font-medium">ID:</span> {localDraft.id}</div>
                      <div><span className="font-medium">Kind:</span> {kindLabel(localDraft.kind)}</div>
                      <div><span className="font-medium">Source:</span> {localDraft.sourceKind}</div>
                    </div>
                    <div>
                      <div><span className="font-medium">URL:</span> {localDraft.sourceUrl.trim().length > 0 ? localDraft.sourceUrl : 'n/a'}</div>
                      <div><span className="font-medium">Data-only policy:</span> required</div>
                      <div><span className="font-medium">Post-install scripts:</span> {postInstallBlocked ? 'detected (blocked)' : 'not declared'}</div>
                    </div>
                  </div>

                  <div className="mt-4 grid grid-cols-1 gap-2 md:grid-cols-3">
                    {[
                      { label: 'Environment', enabled: localDraft.security.env },
                      { label: 'Network', enabled: localDraft.security.network },
                      { label: 'Filesystem', enabled: localDraft.security.fs },
                    ].map((permission) => (
                      <div key={permission.label} className="rounded-lg border border-slate-200 bg-slate-50 p-3 text-sm">
                        <div className="font-medium text-slate-800">{permission.label}</div>
                        <div className={permission.enabled ? 'text-amber-700' : 'text-emerald-700'}>
                          {permission.enabled ? 'Requested' : 'Not requested'}
                        </div>
                      </div>
                    ))}
                  </div>

                  <ul className="mt-4 list-disc space-y-1 pl-5 text-sm text-slate-700">
                    {deriveRiskTags(localDraft.security).map((riskTag) => (
                      <li key={riskTag}>{riskTag}</li>
                    ))}
                  </ul>

                  {postInstallBlocked && (
                    <p className="mt-3 rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-sm text-rose-700">
                      Installation blocked: remote packages must be data-only and cannot include post-install scripts.
                    </p>
                  )}
                </div>
              )}

              {reviewTab === 'configuration' && (
                <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                  <label className="flex flex-col gap-1 text-xs font-medium uppercase tracking-wide text-slate-500">
                    ID
                    <input className="rounded-lg border border-slate-300 px-3 py-2 text-sm text-slate-900 outline-none focus:border-slate-900" onChange={(event) => updateDraft((current) => ({ ...current, id: event.target.value }))} value={localDraft.id} />
                  </label>
                  <label className="flex flex-col gap-1 text-xs font-medium uppercase tracking-wide text-slate-500">
                    Name
                    <input className="rounded-lg border border-slate-300 px-3 py-2 text-sm text-slate-900 outline-none focus:border-slate-900" onChange={(event) => updateDraft((current) => ({ ...current, name: event.target.value }))} value={localDraft.name} />
                  </label>
                  <label className="flex flex-col gap-1 text-xs font-medium uppercase tracking-wide text-slate-500">
                    Kind
                    <select className="rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 outline-none focus:border-slate-900" onChange={(event) => updateDraft((current) => ({ ...current, kind: event.target.value as CatalogKind }))} value={localDraft.kind}>
                      <option value="skill">Skill</option>
                      <option value="agent">Agent</option>
                      <option value="mcp">MCP</option>
                    </select>
                  </label>
                  <label className="flex flex-col gap-1 text-xs font-medium uppercase tracking-wide text-slate-500">
                    Source Kind
                    <select className="rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 outline-none focus:border-slate-900" onChange={(event) => updateDraft((current) => ({ ...current, sourceKind: event.target.value as SourceKind }))} value={localDraft.sourceKind}>
                      <option value="remote">remote</option>
                      <option value="registry">registry</option>
                      <option value="builtin">builtin</option>
                    </select>
                  </label>
                  <label className="flex flex-col gap-1 text-xs font-medium uppercase tracking-wide text-slate-500 md:col-span-2">
                    URL
                    <input className="rounded-lg border border-slate-300 px-3 py-2 text-sm text-slate-900 outline-none focus:border-slate-900" onChange={(event) => updateDraft((current) => ({ ...current, sourceUrl: event.target.value }))} placeholder="https://example.com/package.git" value={localDraft.sourceUrl} />
                  </label>
                  <label className="flex flex-col gap-1 text-xs font-medium uppercase tracking-wide text-slate-500 md:col-span-2">
                    Tool Compatibility (comma-separated)
                    <input className="rounded-lg border border-slate-300 px-3 py-2 text-sm text-slate-900 outline-none focus:border-slate-900" onChange={(event) => updateDraft((current) => ({ ...current, toolCompatibilityText: event.target.value }))} value={localDraft.toolCompatibilityText} />
                  </label>
                  <label className="flex items-center gap-2 text-sm text-slate-700">
                    <input checked={localDraft.verified} onChange={(event) => updateDraft((current) => ({ ...current, verified: event.target.checked }))} type="checkbox" />
                    Verified package
                  </label>
                  <div className="flex flex-wrap gap-3 text-sm text-slate-700 md:col-span-2">
                    <label className="flex items-center gap-2"><input checked={localDraft.security.env} onChange={(event) => updateDraft((current) => ({ ...current, security: { ...current.security, env: event.target.checked } }))} type="checkbox" />Environment access</label>
                    <label className="flex items-center gap-2"><input checked={localDraft.security.network} onChange={(event) => updateDraft((current) => ({ ...current, security: { ...current.security, network: event.target.checked } }))} type="checkbox" />Network access</label>
                    <label className="flex items-center gap-2"><input checked={localDraft.security.fs} onChange={(event) => updateDraft((current) => ({ ...current, security: { ...current.security, fs: event.target.checked } }))} type="checkbox" />Filesystem access</label>
                  </div>
                  <label className="flex flex-col gap-1 text-xs font-medium uppercase tracking-wide text-slate-500 md:col-span-2">
                    Configuration (JSON)
                    <textarea className="min-h-28 rounded-lg border border-slate-300 px-3 py-2 font-mono text-xs text-slate-900 outline-none focus:border-slate-900" onChange={(event) => updateDraft((current) => ({ ...current, configurationText: event.target.value }))} value={localDraft.configurationText} />
                  </label>
                </div>
              )}

              {reviewTab === 'manifest' && (
                <div>
                  <p className="mb-2 text-sm text-slate-600">Raw `macc.package.json` manifest.</p>
                  <textarea
                    aria-label="Manifest editor"
                    className="min-h-72 w-full rounded-lg border border-slate-300 px-3 py-2 font-mono text-xs text-slate-900 outline-none focus:border-slate-900"
                    onChange={(event) => {
                      const value = event.target.value;
                      setManifestText(value);
                      try {
                        const parsed = JSON.parse(value) as unknown;
                        if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
                          setManifestError('Manifest must be a JSON object.');
                          return;
                        }
                        setManifestError(null);
                      } catch {
                        setManifestError('Manifest contains invalid JSON.');
                      }
                    }}
                    value={manifestText}
                  />
                  {manifestError && <p className="mt-2 text-sm text-rose-700">{manifestError}</p>}
                </div>
              )}

              <div className="flex items-center justify-between">
                <p className="text-xs text-slate-500">Install uses config update (`selectedSkills`, `selectedAgents`, `selectedMcp`).</p>
                <div className="flex items-center gap-2">
                  <Button className="border-slate-300 bg-slate-100 text-slate-800 hover:bg-slate-200" onClick={onClose}>Cancel</Button>
                  <Button
                    className="border-slate-900 bg-slate-900 text-white hover:bg-slate-700"
                    disabled={
                      isSaving ||
                      Boolean(manifestError) ||
                      postInstallBlocked ||
                      !localDraft.id.trim() ||
                      (localDraft.sourceKind === 'remote' && localDraft.sourceUrl.trim().length === 0)
                    }
                    onClick={() => onInstall(manifestText, localDraft)}
                  >
                    {isSaving ? 'Installing...' : 'Install'}
                  </Button>
                </div>
              </div>
            </div>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
