import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { ApiSupervisorLive } from '../api/models';
import { SupervisorRow } from './SupervisorRow';
import { elapsedLabel } from './elapsedLabel';

const now = new Date('2026-10-12T02:21:30Z');
const base: ApiSupervisorLive = {
  running: true,
  active: true,
  status: 'validating',
  attempt: 1,
  incident_id: 'run-1-2',
  task_ids: ['WEB-BFF-002'],
  tool: 'claude',
  activity: 'Validation 3/5: pnpm run test',
  started_at: '2026-10-12T02:00:00Z',
  phase_started_at: '2026-10-12T02:17:00Z',
  diagnosis: 'scope gap',
  reason: null,
  next_action: null,
  target_branch: 'feat/p0p1',
  worktree: null,
  log_dir: null,
  findings: ['Improve X', 'Improve Y'],
};

describe('SupervisorRow', () => {
  it('labels the row as supervisor work and shows the current step', () => {
    render(<ul><SupervisorRow supervisor={base} now={now} /></ul>);

    expect(screen.getByText('Supervisor')).toBeInTheDocument();
    expect(screen.getByText('repair WEB-BFF-002')).toBeInTheDocument();
    expect(screen.getByText('validating')).toBeInTheDocument();
    expect(screen.getByText('Doing: Validation 3/5: pnpm run test')).toBeInTheDocument();
    expect(screen.getByText('4m / 21m')).toBeInTheDocument();
    expect(screen.queryByText(/^Reason:/)).not.toBeInTheDocument();
  });

  it('shows the reason and next step when the intervention waits', () => {
    render(
      <ul>
        <SupervisorRow
          supervisor={{
            ...base,
            active: false,
            status: 'escalated',
            activity: null,
            task_ids: [],
            reason: 'Validation failed: pnpm run test',
            next_action: 'Fix the failing command',
          }}
          now={now}
        />
      </ul>,
    );

    expect(screen.getByText('incident run-1-2')).toBeInTheDocument();
    expect(screen.getByText('Reason: Validation failed: pnpm run test')).toBeInTheDocument();
    expect(screen.getByText('Next: Fix the failing command')).toBeInTheDocument();
  });

  it('formats elapsed time and tolerates missing timestamps', () => {
    expect(elapsedLabel('2026-10-12T02:21:00Z', now)).toBe('30s');
    expect(elapsedLabel('2026-10-12T01:16:30Z', now)).toBe('1h05m');
    expect(elapsedLabel(null, now)).toBe('-');
  });
});
