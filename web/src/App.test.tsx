import React from 'react';
import { render, screen, waitFor } from '@testing-library/react';
import { Outlet } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ApiCoordinatorStatus } from './api/models';
import App from './App';

const getStatusMock = vi.fn();

vi.mock('./api/client', () => ({
  getStatus: (...args: unknown[]) => getStatusMock(...args),
}));

vi.mock('./components/Layout', () => ({
  default: () => (
    <React.Suspense fallback={<div>Loading...</div>}>
      <Outlet />
    </React.Suspense>
  ),
}));

vi.mock('./pages/Welcome', () => ({
  default: () => <div>Welcome Stub</div>,
}));

vi.mock('./pages/Dashboard', () => ({
  default: () => <div>Dashboard Stub</div>,
}));

function buildStatus(overrides: Partial<ApiCoordinatorStatus> = {}): ApiCoordinatorStatus {
  return {
    total: 0,
    todo: 0,
    active: 0,
    blocked: 0,
    merged: 0,
    paused: false,
    pause_reason: null,
    pause_task_id: null,
    pause_phase: null,
    latest_error: null,
    failure_report: null,
    throttled_tools: [],
    effective_max_parallel: 3,
    ...overrides,
  };
}

describe('App routing', () => {
  beforeEach(() => {
    getStatusMock.mockReset();
    window.history.replaceState({}, '', '/');
  });

  it('routes the root path to welcome when the project is not initialized', async () => {
    getStatusMock.mockResolvedValue(buildStatus());

    render(<App />);

    expect(await screen.findByText('Welcome Stub')).toBeInTheDocument();
    expect(screen.queryByText('Dashboard Stub')).not.toBeInTheDocument();
  });

  it('routes the root path to dashboard when the project is initialized', async () => {
    getStatusMock.mockResolvedValue(buildStatus({ total: 1, active: 1 }));

    render(<App />);

    await waitFor(() => {
      expect(screen.getByText('Dashboard Stub')).toBeInTheDocument();
    });
    expect(screen.queryByText('Welcome Stub')).not.toBeInTheDocument();
  });
});
