import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import Memory from './Memory';
import { o2Action } from '../../../api/o2';
vi.mock('../../../api/o2', () => ({ o2Action: vi.fn() }));
it('captures source evidence and submits a proposal without promoting it', async () => {
  vi.mocked(o2Action).mockImplementation(async action => action === 'memory_evidence' ? { kind: 'file', reference: 'contract.rs', sha256: 'source-hash', revision: 'head', summary: 'source' } : action === 'memory_search' ? [] : { status: 'proposed' });
  render(<Memory />);
  fireEvent.change(screen.getByLabelText('Memory title'), { target: { value: 'Contract' } });
  fireEvent.change(screen.getByLabelText('Memory body'), { target: { value: 'Keep the public API stable.' } });
  fireEvent.change(screen.getByLabelText('Evidence source path'), { target: { value: 'contract.rs' } });
  fireEvent.click(screen.getByRole('button', { name: 'Submit memory proposal' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('memory_propose', { proposal: expect.objectContaining({ title: 'Contract', body: 'Keep the public API stable.', scope: 'project', source_revision: 'head', evidence: [expect.objectContaining({ sha256: 'source-hash' })] }) }));
  expect(o2Action).not.toHaveBeenCalledWith('memory_review', expect.anything());
});
