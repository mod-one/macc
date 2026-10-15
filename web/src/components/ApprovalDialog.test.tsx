import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { ApiCoordinatorStatus, ApiPendingApproval } from '../api/models';
import { useCoordinatorStore } from '../store';
import { useNotificationStore } from '../stores/notificationStore';
import { ApprovalCenter } from './ApprovalDialog';

const api = vi.hoisted(() => ({ list: vi.fn(), decide: vi.fn(), status: vi.fn() }));
vi.mock('../api/client', () => ({
  getApprovals: api.list,
  postGateDecision: api.decide,
  getStatus: api.status,
  postCoordinatorAction: vi.fn(),
}));

function gate(overrides: Partial<ApiPendingApproval> = {}): ApiPendingApproval {
  return {
    task_id: 'FIN-APP-001',
    title: 'Approve the FIN-CR-001 normative contract',
    state: 'waiting_approval',
    status: 'waiting_approval',
    subject_task: 'FIN-CR-001',
    subject_revision: 'abcdef1234567890',
    description: 'Adopt the FIN-CR-001 normative contract',
    risks: ['A wrong contract propagates into the OpenAPI.'],
    approval_trigger: 'normative-change',
    governance_ref: 'docs/16-decisions.md',
    required: 'FINANCE 0/1, LEAD_TECHNIQUE 0/1',
    missing_roles: ['FINANCE', 'LEAD_TECHNIQUE'],
    evidence_type: 'pull_request_review',
    note: 'waiting for approval of FIN-CR-001',
    diff_stat: ' docs/12-api.md | 40 +',
    approve_command: 'macc coordinator approve FIN-APP-001 --role FINANCE --revision abcdef1 --evidence <url>',
    question: 'Do you approve FIN-CR-001 ("Specify settlement endpoints") as delivered at revision abcdef123456?',
    awaiting_decision: true,
    subject_title: 'Specify settlement endpoints',
    subject_objective: 'Close the contract gap.',
    subject_description: 'Problem/Goal: contract gap.',
    subject_result: 'New EP-* rows.',
    subject_acceptance_criteria: ['docs/12-api.md lists the new rows.'],
    required_roles: ['FINANCE', 'LEAD_TECHNIQUE'],
    quorum: 'every listed role must approve: one decision per role',
    evidence_required: true,
    expires_after_days: null,
    invalidate_on_subject_change: true,
    decisions: [],
    unblocks: ['FIN-API-002 — Settlement endpoints'],
    ...overrides,
  };
}

function setPending(pending: ApiPendingApproval[]) {
  useCoordinatorStore.setState({
    status: { pending_approvals: pending } as unknown as ApiCoordinatorStatus,
  });
}

beforeEach(() => {
  window.sessionStorage.clear();
  api.list.mockResolvedValue({ pending_approvals: [], default_actor: 'Ann <ann@x>' });
  api.status.mockResolvedValue({ pending_approvals: [] });
  api.decide.mockResolvedValue({ gate_decision: 'Recorded approved for FIN-APP-001 as FINANCE.', pending_approvals: [] });
});
afterEach(() => {
  vi.clearAllMocks();
  useCoordinatorStore.setState({ status: null });
});

it('opens by itself with the full explanation of the question', async () => {
  setPending([gate()]);
  await act(async () => {
    render(<ApprovalCenter />);
  });
  expect(screen.getByRole('dialog')).toBeInTheDocument();
  expect(screen.getByText(/Do you approve FIN-CR-001/)).toBeInTheDocument();
  expect(screen.getByText('Objective: Close the contract gap.')).toBeInTheDocument();
  expect(screen.getByText('docs/12-api.md lists the new rows.')).toBeInTheDocument();
  expect(screen.getByText('A wrong contract propagates into the OpenAPI.')).toBeInTheDocument();
  expect(screen.getByText(/the run continues with FIN-API-002/)).toBeInTheDocument();
});

it('requires evidence, confirms, then records the approval for the shown revision', async () => {
  setPending([gate()]);
  await act(async () => {
    render(<ApprovalCenter />);
  });
  const approve = screen.getByRole('button', { name: 'Approve…' });
  expect(approve).toBeDisabled();
  fireEvent.change(screen.getByPlaceholderText(/pull request review/), { target: { value: 'https://git/pr/7' } });
  fireEvent.click(screen.getByRole('button', { name: 'Approve…' }));
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));
  });
  expect(api.decide).toHaveBeenCalledWith('FIN-APP-001', {
    decision: 'approved',
    role: 'FINANCE',
    revision: 'abcdef1234567890',
    evidence: 'https://git/pr/7',
    reason: undefined,
    actor: undefined,
  });
  // The gate is resolved: the dialog closes and the outcome stays in the notification center.
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  expect(useNotificationStore.getState().notifications[0].message).toBe(
    'Recorded approved for FIN-APP-001 as FINANCE.',
  );
});

it('stays open with the outcome while other roles are still needed', async () => {
  setPending([gate()]);
  api.status.mockResolvedValue({ pending_approvals: [gate({ missing_roles: ['LEAD_TECHNIQUE'] })] });
  await act(async () => {
    render(<ApprovalCenter />);
  });
  fireEvent.change(screen.getByPlaceholderText(/pull request review/), { target: { value: 'https://git/pr/7' } });
  fireEvent.click(screen.getByRole('button', { name: 'Approve…' }));
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));
  });
  expect(screen.getByRole('dialog')).toBeInTheDocument();
  expect(screen.getByRole('status')).toHaveTextContent('Recorded approved for FIN-APP-001 as FINANCE.');
});

it('a rejection needs a reason', async () => {
  setPending([gate()]);
  await act(async () => {
    render(<ApprovalCenter />);
  });
  fireEvent.click(screen.getByText('Reject'));
  expect(screen.getByRole('button', { name: 'Reject…' })).toBeDisabled();
  fireEvent.change(screen.getByRole('textbox', { name: /Reason/ }), { target: { value: 'Missing ETag' } });
  fireEvent.click(screen.getByRole('button', { name: 'Reject…' }));
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));
  });
  expect(api.decide).toHaveBeenCalledWith('FIN-APP-001', expect.objectContaining({ decision: 'rejected', reason: 'Missing ETag' }));
});

it('"Decide later" closes the dialog until the revision changes; the pill re-opens it', async () => {
  setPending([gate()]);
  await act(async () => {
    render(<ApprovalCenter />);
  });
  fireEvent.click(screen.getByRole('button', { name: 'Decide later' }));
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();

  await act(async () => {
    setPending([gate({ subject_revision: '1111111222222' })]);
  });
  expect(screen.getByRole('dialog')).toBeInTheDocument();

  fireEvent.click(screen.getByRole('button', { name: 'Decide later' }));
  fireEvent.click(screen.getByRole('button', { name: '1 approval waiting' }));
  expect(screen.getByRole('dialog')).toBeInTheDocument();
});

it('shows nothing when no gate awaits a decision', async () => {
  setPending([gate({ awaiting_decision: false, status: 'rejected' })]);
  await act(async () => {
    render(<ApprovalCenter />);
  });
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  expect(screen.queryByText(/approval waiting/)).not.toBeInTheDocument();
});
