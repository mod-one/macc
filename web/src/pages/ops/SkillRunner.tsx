import React, { useEffect, useState } from 'react';
import { buildUrl } from '../../api/client';
import { Button } from '../../components/Button';
import { LoadingSpinner } from '../../components/LoadingSpinner';
import { ErrorBanner } from '../../components/ErrorBanner';
import { StatusBadge } from '../../components/StatusBadge';

interface SkillItem {
  id: string;
  title: string;
  kind: string;
  risk: string;
  description: string;
}

interface DryRunPreview {
  skillId: string;
  title: string;
  kind: string;
  tool: string | null;
  risk: string;
  commands: string[];
  writes: string[];
  contextEstimate: string | null;
  logsPath: string;
}

interface RunResult {
  skillId: string;
  status: string;
  tool: string | null;
  startedAt: string;
  durationMs: number;
  stdout: string;
  stderr: string;
  exitCode: number | null;
}

const RISK_COLORS: Record<string, string> = {
  safe: 'text-green-400',
  caution: 'text-yellow-400',
  dangerous: 'text-red-400',
};

const SkillRunner: React.FC = () => {
  const [skills, setSkills] = useState<SkillItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<SkillItem | null>(null);
  const [preview, setPreview] = useState<DryRunPreview | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<RunResult | null>(null);
  const [runError, setRunError] = useState<string | null>(null);

  const fetchSkills = async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(buildUrl('/skills'));
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      setSkills(Array.isArray(data) ? data : []);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load skills');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void fetchSkills();
  }, []);

  const handleSelect = async (skill: SkillItem) => {
    setSelected(skill);
    setPreview(null);
    setResult(null);
    setRunError(null);
    setPreviewLoading(true);
    try {
      const res = await fetch(buildUrl(`/skills/${encodeURIComponent(skill.id)}/dry-run`), {
        method: 'POST',
      });
      if (res.ok) {
        const data = await res.json() as DryRunPreview;
        setPreview(data);
      }
    } catch {
      // preview is optional
    } finally {
      setPreviewLoading(false);
    }
  };

  const handleRun = async () => {
    if (!selected) return;
    setRunning(true);
    setResult(null);
    setRunError(null);
    try {
      const res = await fetch(buildUrl(`/skills/${encodeURIComponent(selected.id)}/run`), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ yes: true }),
      });
      if (!res.ok) {
        const err = await res.text();
        setRunError(`Run failed (${res.status}): ${err}`);
        return;
      }
      const data = await res.json() as RunResult;
      setResult(data);
    } catch (e) {
      setRunError(e instanceof Error ? e.message : 'Run failed');
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="flex h-full gap-4 p-4">
      {/* Left: skill catalog */}
      <div className="w-72 shrink-0 flex flex-col gap-2">
        <div className="text-sm font-semibold text-zinc-300 mb-1">Skills</div>
        {loading && <LoadingSpinner />}
        {error && <ErrorBanner message={error} />}
        {!loading &&
          skills.map((s) => (
            <button
              key={s.id}
              onClick={() => void handleSelect(s)}
              className={`text-left px-3 py-2 rounded border transition-colors ${
                selected?.id === s.id
                  ? 'border-zinc-400 bg-zinc-700 text-white'
                  : 'border-zinc-700 bg-zinc-800 text-zinc-300 hover:border-zinc-500'
              }`}
            >
              <div className="flex items-center gap-2">
                <span className={`text-xs font-mono ${RISK_COLORS[s.risk] ?? 'text-zinc-400'}`}>
                  [{s.risk}]
                </span>
                <span className="font-mono text-sm">{s.id}</span>
              </div>
              <div className="text-xs text-zinc-500 mt-0.5 truncate">{s.title}</div>
            </button>
          ))}
        {!loading && skills.length === 0 && (
          <div className="text-xs text-zinc-500">
            No skills found. Add YAML files to .macc/skills/
          </div>
        )}
      </div>

      {/* Right: skill detail + run */}
      <div className="flex-1 flex flex-col gap-4 min-w-0">
        {!selected && (
          <div className="text-zinc-500 text-sm mt-8 text-center">
            Select a skill from the list to view details and run it.
          </div>
        )}

        {selected && (
          <>
            <div className="bg-zinc-800 border border-zinc-700 rounded p-4">
              <div className="flex items-center gap-3 mb-2">
                <span className="font-mono font-bold text-white text-lg">{selected.id}</span>
                <span
                  className={`text-xs px-2 py-0.5 rounded font-mono ${RISK_COLORS[selected.risk] ?? ''}`}
                >
                  {selected.risk}
                </span>
                <span className="text-xs text-zinc-400 font-mono">{selected.kind}</span>
              </div>
              <div className="text-sm text-zinc-300">{selected.title}</div>
              {selected.description && (
                <div className="text-xs text-zinc-500 mt-1">{selected.description}</div>
              )}
            </div>

            {/* Dry-run preview */}
            {previewLoading && <LoadingSpinner />}
            {preview && !previewLoading && (
              <div className="bg-zinc-900 border border-zinc-700 rounded p-4">
                <div className="text-xs font-semibold text-zinc-400 mb-2">Dry-run preview</div>
                {preview.commands.length > 0 && (
                  <div className="mb-2">
                    <div className="text-xs text-zinc-500 mb-1">Commands:</div>
                    {preview.commands.map((cmd, i) => (
                      <div key={i} className="font-mono text-xs text-zinc-300 pl-2">
                        $ {cmd}
                      </div>
                    ))}
                  </div>
                )}
                <div className="text-xs text-zinc-600 mt-2">Logs → {preview.logsPath}</div>
              </div>
            )}

            {/* Run controls */}
            <div className="flex gap-3">
              <Button
                onClick={() => void handleRun()}
                disabled={running}
                variant="primary"
              >
                {running ? 'Running…' : 'Run'}
              </Button>
              <Button
                onClick={() => void handleSelect(selected)}
                disabled={previewLoading}
                variant="secondary"
              >
                Dry Run
              </Button>
            </div>

            {runError && <ErrorBanner message={runError} />}

            {/* Run result */}
            {result && (
              <div className="bg-zinc-900 border border-zinc-700 rounded p-4">
                <div className="flex items-center gap-3 mb-2">
                  <StatusBadge status={result.status} />
                  <span className="text-xs text-zinc-400">{result.durationMs}ms</span>
                </div>
                {result.stdout && (
                  <pre className="text-xs text-zinc-300 bg-zinc-950 p-2 rounded overflow-auto max-h-48 whitespace-pre-wrap">
                    {result.stdout}
                  </pre>
                )}
                {result.stderr && (
                  <pre className="text-xs text-red-400 bg-zinc-950 p-2 rounded overflow-auto max-h-24 mt-2 whitespace-pre-wrap">
                    {result.stderr}
                  </pre>
                )}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
};

export default SkillRunner;
