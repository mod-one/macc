import React, { useEffect, useState } from 'react';
import { getHealth } from '../api/client';
import type { ApiHealthResponse } from '../api/models';

const About: React.FC = () => {
  const [health, setHealth] = useState<ApiHealthResponse | null>(null);

  useEffect(() => {
    getHealth()
      .then(setHealth)
      .catch(() => null);
  }, []);

  const version = health?.version ?? '0.2.0';
  const buildSha = health?.build_sha ?? 'unknown';
  const buildVersion = health?.build_version ?? `${version} (build ${buildSha})`;

  return (
    <div className="flex flex-col gap-6">
      <header className="rounded-[2rem] border border-slate-200 bg-white p-6 shadow-sm">
        <h1 className="text-5xl font-semibold tracking-tight text-slate-950">About MACC</h1>
        <p className="mt-3 text-base text-slate-600">
          Multi-Agentic Coding Config — local orchestration, worktree automation, and observability.
        </p>
        <dl className="mt-6 grid grid-cols-1 gap-4 sm:grid-cols-3">
          <div className="rounded-xl border border-slate-100 bg-slate-50 p-4">
            <dt className="text-xs font-medium uppercase tracking-wider text-slate-500">Version</dt>
            <dd className="mt-1 font-mono text-lg font-semibold text-slate-900">{buildVersion}</dd>
          </div>
          <div className="rounded-xl border border-slate-100 bg-slate-50 p-4">
            <dt className="text-xs font-medium uppercase tracking-wider text-slate-500">Build SHA</dt>
            <dd className="mt-1 font-mono text-lg font-semibold text-slate-900">{buildSha}</dd>
          </div>
          <div className="rounded-xl border border-slate-100 bg-slate-50 p-4">
            <dt className="text-xs font-medium uppercase tracking-wider text-slate-500">API Status</dt>
            <dd className="mt-1 font-mono text-lg font-semibold text-slate-900">
              {health?.status ?? 'loading'}
            </dd>
          </div>
        </dl>
      </header>
    </div>
  );
};

export default About;

