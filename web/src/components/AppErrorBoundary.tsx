import React from 'react';

interface AppErrorBoundaryState {
  error: Error | null;
}

export class AppErrorBoundary extends React.Component<
  React.PropsWithChildren,
  AppErrorBoundaryState
> {
  state: AppErrorBoundaryState = {
    error: null,
  };

  static getDerivedStateFromError(error: Error): AppErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo): void {
    console.error('MACC web client crashed', error, errorInfo);
  }

  render(): React.ReactNode {
    if (!this.state.error) {
      return this.props.children;
    }

    return (
      <main className="flex min-h-screen items-center justify-center bg-[var(--bg-primary)] px-6 text-[var(--text-primary)]">
        <section className="w-full max-w-2xl rounded-2xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6 shadow-xl">
          <p className="text-sm font-semibold uppercase tracking-[0.2em] text-[var(--text-secondary)]">
            Web Client Error
          </p>
          <h1 className="mt-3 text-3xl font-semibold">The page crashed while rendering.</h1>
          <p className="mt-3 text-sm text-[var(--text-secondary)]">
            This usually means a runtime error in the SPA or a missing production asset chunk.
            Rebuild with <code>npm run build</code> before <code>cargo build --release</code>,
            then refresh the page.
          </p>
          <pre className="mt-5 overflow-auto rounded-xl border border-[var(--border)] bg-[var(--bg-card)] p-4 text-xs text-[var(--text-secondary)]">
            {this.state.error.stack ?? this.state.error.message}
          </pre>
        </section>
      </main>
    );
  }
}
