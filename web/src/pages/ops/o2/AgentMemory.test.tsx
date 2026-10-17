import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import AgentMemory from './AgentMemory';
import { o2Action } from '../../../api/o2';
vi.mock('../../../api/o2', () => ({ o2Action: vi.fn() }));
it('inspects redacted MCP setup and extracts proposals without activating memories', async () => {
  vi.mocked(o2Action).mockResolvedValue({ status: 'proposed' });
  const refresh = vi.fn().mockResolvedValue(undefined);
  render(<AgentMemory onExtracted={refresh} />);
  fireEvent.click(screen.getByText('Agent access and phase proposals'));
  fireEvent.click(screen.getByRole('button', { name: 'Inspect scoped MCP setup' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('agent_status'));
  fireEvent.change(screen.getByLabelText('Extraction checkpoint ID'), { target: { value: 'cp-1' } });
  fireEvent.click(screen.getByRole('button', { name: 'Extract phase proposals' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('memory_extract', { checkpoint_id: 'cp-1' }));
  expect(refresh).toHaveBeenCalled();
  expect(o2Action).not.toHaveBeenCalledWith('memory_review', expect.anything());
});
