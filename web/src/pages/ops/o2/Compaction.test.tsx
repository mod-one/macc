import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import Compaction from './Compaction';
import { o2Action } from '../../../api/o2';
import { getSessions } from '../../../api/sessions';
vi.mock('../../../api/o2', () => ({ o2Action: vi.fn() }));
vi.mock('../../../api/sessions', () => ({ getSessions: vi.fn() }));
beforeEach(() => vi.resetAllMocks());
it('reviews existing native ownership before adopting the exact preview', async () => {
  vi.mocked(getSessions).mockResolvedValue({ sessions: [{ id: 'session-a', task_id: 'A', role: 'performer', tool_id: 'future', status: 'available' }] } as Awaited<ReturnType<typeof getSessions>>);
  vi.mocked(o2Action).mockImplementation(async action => action === 'compaction_list' ? [] : action === 'native_setting_preview' ? { checksum: 'reviewed', plan: { current_value: 5000 } } : { adopted: true });
  render(<Compaction />);
  fireEvent.click(screen.getByRole('button', { name: 'Refresh compaction capabilities' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Review native setting ownership for A' }));
  await screen.findByRole('button', { name: 'Confirm native setting ownership' });
  expect(o2Action).not.toHaveBeenCalledWith('native_setting_adopt', expect.anything());
  fireEvent.click(screen.getByRole('button', { name: 'Confirm native setting ownership' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('native_setting_adopt', { id: 'session-a', checksum: 'reviewed' }));
});
