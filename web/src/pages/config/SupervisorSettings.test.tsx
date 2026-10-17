import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import type { ApiConfigResponse, SupervisorConfig, SupervisorField } from '../../api/models';
import SupervisorSettings from './SupervisorSettings';
import { supervisorErrors } from './supervisorValidation';
vi.mock('../../api/client', () => ({ getToolDescriptors: vi.fn().mockResolvedValue([{ id: 'fake', fields: [{ id: 'model', kind: { type: 'enum', options: ['small', 'large'] } }] }]) }));
const config: SupervisorConfig = {
  tool: null, model: null, effort: null, repair_enabled: true, tests_enabled: true,
  ai_verification_enabled: true, auto_restart: true, validation_commands: [],
  intervention_timeout_seconds: 3600, validation_timeout_seconds: 3600, max_repair_rounds: 3,
  max_restart_attempts: 3, watchdog_interval_seconds: 30, log_analysis_window_seconds: 300,
  report_output_path: '.macc/log/supervisor/report.json', crash_debounce_checks: 3, events_log_path: '.macc/log/coordinator/events.jsonl',
};
const fields: SupervisorField[] = [
  { key: 'tool', label: 'AI tool', kind: 'optional', help: 'ToolSpec tool' },
  { key: 'tests_enabled', label: 'Tests and checks', kind: 'bool', help: 'Run tests' },
  { key: 'ai_verification_enabled', label: 'AI verification', kind: 'bool', help: 'Review changes' },
  { key: 'max_repair_rounds', label: 'Repair rounds', kind: 'number', help: 'At least one' },
];
function Harness() {
  const [supervisor, setSupervisor] = useState(config);
  return <SupervisorSettings draft={{ supervisor, supervisorFields: fields, enabledTools: ['fake'] } as ApiConfigResponse} update={patch => setSupervisor(patch.supervisor!)} />;
}
describe('Supervisor settings', () => {
  it('supports tool selection, command rows and independent phase toggles', async () => {
    render(<Harness />);
    await waitFor(() => expect(screen.getByText(/Effective tool: fake/)).toBeInTheDocument());
    fireEvent.change(screen.getByLabelText('AI tool'), { target: { value: 'fake' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add validation command' }));
    expect(screen.getByRole('alert')).toHaveTextContent('Remove or fill empty validation commands');
    fireEvent.change(screen.getByLabelText('Validation command 1'), { target: { value: 'cd web && npm test' } });
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Tests and checks'));
    expect(screen.getByLabelText('Validation command 1')).toBeEnabled();
    expect(screen.getByLabelText('AI verification')).toBeChecked();
    expect(screen.getByRole('status')).toHaveTextContent('Tests are skipped');
    fireEvent.click(screen.getByLabelText('AI verification'));
    expect(screen.getByRole('status')).toHaveTextContent('AI verification is skipped');
  });
  it('rejects invalid bounds and paths without accepting empty commands', () => {
    expect(supervisorErrors({ ...config, max_repair_rounds: 0, report_output_path: '../outside.json', validation_commands: [''] })).toHaveLength(3);
    expect(supervisorErrors(config)).toEqual([]);
  });
});
