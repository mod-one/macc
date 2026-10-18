import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import axe from 'axe-core';
import userEvent from '@testing-library/user-event';
import MemoryTransfer from './MemoryTransfer';
import { o2Action } from '../../../api/o2';
vi.mock('../../../api/o2', () => ({ o2Action: vi.fn() }));
it('requires preview and explicit confirmation before importing proposals', async () => {
  vi.mocked(o2Action).mockImplementation(async action => action.endsWith('_preview') ? { checksum: 'reviewed', count: 1, records: [{ status: 'proposed', scope: 'project' }], notice: 'Review locally' } : { count: 1 });
  render(<MemoryTransfer identity="A" access={{ task_id: 'A' }} />);
  fireEvent.change(screen.getByLabelText('Transfer operation'), { target: { value: 'import' } });
  fireEvent.change(screen.getByLabelText('Transfer document'), { target: { value: '{"record":"source"}' } });
  fireEvent.click(screen.getByRole('button', { name: 'Preview memory transfer' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Apply reviewed memory transfer' })).toBeDisabled());
  expect(o2Action).toHaveBeenCalledWith('memory_import_preview', { request: { document: '{"record":"source"}', access: { task_id: 'A' }, format: 'jsonl' } });
  fireEvent.click(screen.getByRole('checkbox'));
  fireEvent.click(screen.getByRole('button', { name: 'Apply reviewed memory transfer' }));
  await waitFor(() => expect(o2Action).toHaveBeenCalledWith('memory_import', { request: expect.anything(), checksum: 'reviewed', confirmed: true }));
  await screen.findByText('Import completed. Inspect each proposal before activation.');
});
it('invalidates the transfer preview when records, format or scope change', async () => {
  vi.mocked(o2Action).mockResolvedValue({ checksum: 'reviewed', count: 1, document: 'source', notice: 'Review' });
  const ui = render(<MemoryTransfer identity="A" access={{ task_id: 'A' }} selectedId="one" />);
  fireEvent.click(screen.getByRole('button', { name: 'Preview memory transfer' }));
  await screen.findByRole('checkbox');
  fireEvent.click(screen.getByRole('checkbox'));
  fireEvent.change(screen.getByLabelText('Transfer format'), { target: { value: 'markdown' } });
  expect(screen.queryByRole('button', { name: 'Apply reviewed memory transfer' })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Preview memory transfer' }));
  await screen.findByRole('checkbox');
  ui.rerender(<MemoryTransfer identity="B" access={{ task_id: 'B' }} selectedId="one" />);
  expect(screen.queryByRole('button', { name: 'Apply reviewed memory transfer' })).toBeNull();
});

it('supports keyboard preview and labels transfer confirmation accessibly', async () => {
  vi.mocked(o2Action).mockResolvedValue({ checksum: 'reviewed', count: 1, document: 'source', notice: 'Review records' });
  const user = userEvent.setup();
  render(<main><MemoryTransfer identity="A" access={{}} selectedId="one" /></main>);
  screen.getByRole('button', { name: 'Preview memory transfer' }).focus();
  await user.keyboard('{Enter}');
  const confirm = await screen.findByRole('checkbox'); confirm.focus(); await user.keyboard(' ');
  expect(screen.getByRole('button', { name: 'Apply reviewed memory transfer' })).toBeEnabled();
  const result = await axe.run(document.body, { rules: { 'color-contrast': { enabled: false } } });
  expect(result.violations).toEqual([]);
});

it('discards a file read completed after the operator changes transfer format', async () => {
  render(<MemoryTransfer identity="A" access={{}} />);
  fireEvent.change(screen.getByLabelText('Transfer operation'), { target: { value: 'import' } });
  let resolve!: (text: string) => void;
  const file = new File(['source'], 'memory.jsonl');
  Object.defineProperty(file, 'text', { value: () => new Promise<string>(done => { resolve = done; }) });
  fireEvent.change(screen.getByLabelText('Memory transfer file'), { target: { files: [file] } });
  fireEvent.change(screen.getByLabelText('Transfer format'), { target: { value: 'markdown' } });
  await act(async () => { resolve('obsolete JSONL content'); });
  expect(screen.getByLabelText('Transfer document')).toHaveValue('');
});
