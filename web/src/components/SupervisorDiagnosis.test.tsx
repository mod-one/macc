import { render, screen } from '@testing-library/react';
import { expect, it } from 'vitest';
import { SupervisorDiagnosis } from './SupervisorDiagnosis';

it('updates the diagnosis and outcome in place when the supervisor finishes', () => {
  const note = { status: 'diagnosing', diagnosis: 'Checking PRD provenance.', task_ids: ['ROOT'] };
  const { rerender } = render(<SupervisorDiagnosis note={note} />);
  expect(screen.getByText(note.diagnosis)).toBeVisible();
  rerender(<SupervisorDiagnosis note={{
    ...note, status: 'ready_to_resume', diagnosis: 'Previous performer changed the PRD.',
    outcome: 'Worker recovered.', next_action: 'Start the coordinator.',
  }} />);
  expect(screen.queryByText(note.diagnosis)).toBeNull();
  expect(screen.getByText('Supervisor · ready_to_resume · ROOT')).toBeVisible();
  expect(screen.getByText('Worker recovered.')).toBeVisible();
  expect(screen.getByText('Supervisor next: Start the coordinator.')).toBeVisible();
});
