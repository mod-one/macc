import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import Providers from './Providers';
import { o2Action } from '../../../api/o2';
vi.mock('../../../api/o2', () => ({ o2Action: vi.fn() }));
it('applies only after inspecting and confirming the returned plan hash', async () => {
  vi.mocked(o2Action).mockImplementation(async action => action === 'provider_plan' ? { result: { outcome: 'supported', data: { id: 'plan-1', affected_paths: ['owned-index'], network: false } }, checksum: 'reviewed-sha' } : action === 'providers' ? { providers: [], receipts: [], plans: [] } : { state: 'deployed' });
  render(<Providers />);
  fireEvent.click(screen.getByRole('button', { name: 'Preview provider operation' }));
  const apply = await screen.findByRole('button', { name: 'Apply reviewed provider operation' });
  expect(apply).toBeDisabled();
  expect(o2Action).not.toHaveBeenCalledWith('provider_apply', expect.anything());
  fireEvent.click(screen.getByLabelText('I reviewed the affected paths, origin and network policy'));
  fireEvent.click(apply);
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('provider_apply', { plan_id: 'plan-1', checksum: 'reviewed-sha' }));
});
