import React from 'react';

const blocks = Array.from({ length: 6 }, (_, index) => index);

const RouteLoadingSkeleton: React.FC = () => (
  <div className="h-full w-full animate-pulse">
    <div className="mx-auto flex h-full w-full max-w-6xl flex-col gap-6">
      <div className="rounded-3xl border border-[var(--border)] bg-[var(--bg-card)] p-6">
        <div className="mb-3 h-4 w-40 rounded bg-[var(--bg-secondary)]" />
        <div className="mb-2 h-8 w-1/3 rounded bg-[var(--bg-secondary)]" />
        <div className="h-4 w-2/3 rounded bg-[var(--bg-secondary)]" />
      </div>
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
        {blocks.map((block) => (
          <div
            key={block}
            className="rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] p-4"
          >
            <div className="mb-3 h-4 w-1/2 rounded bg-[var(--bg-secondary)]" />
            <div className="mb-2 h-3 w-full rounded bg-[var(--bg-secondary)]" />
            <div className="mb-2 h-3 w-5/6 rounded bg-[var(--bg-secondary)]" />
            <div className="h-3 w-2/3 rounded bg-[var(--bg-secondary)]" />
          </div>
        ))}
      </div>
    </div>
  </div>
);

export default RouteLoadingSkeleton;
