import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ApiTrustSummary } from '../../api/models';
import Trust from './Trust';

const getTrustMock = vi.fn();

vi.mock('../../api/client', () => ({
  getTrust: (...args: unknown[]) => getTrustMock(...args),
}));

function buildTrustSummary(overrides: Partial<ApiTrustSummary> = {}): ApiTrustSummary {
  return {
    state: 'trusted',
    local_only: true,
    terminal_enabled: false,
    user_level_writes: 0,
    backups_ready: true,
    catalog_pinned: true,
    secrets_redacted: true,
    server_exposure: '127.0.0.1:3450',
    allowed_roots: ['/home/brand/macc'],
    audit_log: '/home/brand/macc/.macc/log/coordinator/coordinator.log',
    ...overrides,
  };
}

describe('Trust & Safety page', () => {
  beforeEach(() => {
    getTrustMock.mockReset();
  });

  it('renders with a normal trust summary', async () => {
    getTrustMock.mockResolvedValue(buildTrustSummary());

    render(<Trust />);

    await screen.findByText('Trust & Safety');
    expect(screen.getByText('Local Only')).toBeInTheDocument();
    expect(screen.getByText('trusted')).toBeInTheDocument();
    expect(screen.getByText('/home/brand/macc/.macc/log/coordinator/coordinator.log')).toBeInTheDocument();
  });
});
