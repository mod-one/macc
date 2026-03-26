import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ApiCoordinatorStatus } from '../api/models';
import Welcome from './Welcome';

const getStatusMock = vi.fn();
const navigateMock = vi.fn();

vi.mock('../api/client', () => ({
  buildUrl: (path: string) => path,
  getStatus: (...args: unknown[]) => getStatusMock(...args),
}));

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom');
  return {
    ...actual,
    useNavigate: () => navigateMock,
  };
});

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

function renderWelcome(): void {
  render(
    <MemoryRouter initialEntries={['/welcome']}>
      <Welcome />
    </MemoryRouter>,
  );
}

describe('Welcome page', () => {
  beforeEach(() => {
    getStatusMock.mockReset();
    navigateMock.mockReset();
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ status: 'ok' }), {
          headers: { 'content-type': 'application/json' },
        }),
      ),
    );
  });

  it('renders the three onboarding cards and quick start action', async () => {
    getStatusMock.mockResolvedValue(buildStatus());
    const user = userEvent.setup();

    renderWelcome();

    expect(await screen.findByRole('heading', { name: /Set up the workspace in three guided steps/i })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Detect & Install Adapters/i })).toHaveAttribute('href', '/config/tools');
    expect(screen.getByRole('link', { name: /Configure Project/i })).toHaveAttribute('href', '/init');
    expect(screen.getByRole('link', { name: /Import Skills/i })).toHaveAttribute('href', '/config/skills');

    await user.click(screen.getByRole('button', { name: 'Quick Start' }));
    expect(navigateMock).toHaveBeenCalledWith('/init');
  });

  it('shows a version badge when health metadata reports a newer release', async () => {
    getStatusMock.mockResolvedValue(buildStatus());
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            status: 'ok',
            currentVersion: '1.0.0',
            latestVersion: '1.1.0',
          }),
          { headers: { 'content-type': 'application/json' } },
        ),
      ),
    );

    renderWelcome();

    expect(await screen.findByText(/New version available 1\.1\.0/i)).toBeInTheDocument();
  });

  it('redirects initialized projects to the dashboard', async () => {
    getStatusMock.mockResolvedValue(buildStatus({ total: 1, active: 1 }));

    renderWelcome();

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/dashboard', { replace: true });
    });
  });
});
