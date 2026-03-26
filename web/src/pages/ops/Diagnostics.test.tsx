import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ApiDoctorReport } from '../../api/models';
import Diagnostics from './Diagnostics';

const getDoctorReportMock = vi.fn();
const runDoctorFixMock = vi.fn();

vi.mock('../../api/client', () => ({
  getDoctorReport: (...args: unknown[]) => getDoctorReportMock(...args),
  runDoctorFix: (...args: unknown[]) => runDoctorFixMock(...args),
}));

function buildReport(overrides: Partial<ApiDoctorReport> = {}): ApiDoctorReport {
  return {
    healthScore: 95,
    issuesBySeverity: {
      error: 0,
      warning: 1,
    },
    issues: [
      {
        name: 'Node.js',
        toolId: null,
        target: 'node',
        severity: 'warning',
        kind: 'which',
        status: 'missing',
        message: 'Node.js is not installed.',
      },
    ],
    ...overrides,
  };
}

describe('Diagnostics page', () => {
  beforeEach(() => {
    getDoctorReportMock.mockReset();
    runDoctorFixMock.mockReset();
    runDoctorFixMock.mockResolvedValue({ status: 'success', message: 'ok' });
  });

  it('renders with a normal doctor report', async () => {
    getDoctorReportMock.mockResolvedValue(buildReport());

    render(<Diagnostics />);

    await screen.findByText('Diagnostics');
    expect(screen.getByText('Node.js')).toBeInTheDocument();
  });

  it('renders when issue collections are missing from the payload', async () => {
    getDoctorReportMock.mockResolvedValue(
      buildReport({
        issues: null as unknown as ApiDoctorReport['issues'],
        issuesBySeverity: null as unknown as ApiDoctorReport['issuesBySeverity'],
      }),
    );

    render(<Diagnostics />);

    await screen.findByText('Diagnostics');
    expect(screen.getByText('No Issues Found')).toBeInTheDocument();
  });
});
