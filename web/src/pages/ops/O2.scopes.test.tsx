import { act, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter, Routes, Route, Link } from 'react-router-dom';
import { expect, it, vi } from 'vitest';
import O2 from './O2';
import { getO2, o2Action, type O2View } from '../../api/o2';
vi.mock('../../api/o2', () => ({ getO2: vi.fn(), o2Action: vi.fn() }));
vi.mock('./o2/Policy', () => ({ default: () => null }));
vi.mock('./o2/Providers', () => ({ default: () => null }));
vi.mock('./o2/Compaction', () => ({ default: () => null }));
vi.mock('./o2/Overview', () => ({ default: () => null }));
it('keeps the actual Memory and Context inspectors scoped across routed domain and task links', async () => {
  vi.mocked(getO2).mockResolvedValue({ project_root: '/repo', revision: 'r', source: 'project', sqlite: true, ready: true, fields: [], policy: {} } as O2View);
  let resolveSearch!: (value: unknown) => void;
  vi.mocked(o2Action).mockImplementation(async action => {
    if (action === 'context_get') return { task_id: 'A', evidence: 'context-A-evidence' };
    if (action === 'memory_search') return new Promise(resolve => { resolveSearch = resolve; });
    return {};
  });
  render(<MemoryRouter initialEntries={['/ops/context?task=A&id=context-a']}>
    <Link to="/ops/memory?task=B">Open task B inbox</Link>
    <Routes><Route path="/ops/:domain" element={<O2 />} /></Routes>
  </MemoryRouter>);
  expect(await screen.findByText(/context-A-evidence/)).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: /^Memory$/ }));
  expect(screen.queryByText(/context-A-evidence/)).not.toBeInTheDocument();
  expect(screen.getByLabelText('Task scope')).toHaveValue('A');
  fireEvent.click(screen.getByRole('button', { name: 'Search memory inbox' }));
  expect(o2Action).toHaveBeenCalledWith('memory_search', { search: expect.objectContaining({ access: expect.objectContaining({ task_id: 'A' }) }) });
  fireEvent.click(screen.getByRole('link', { name: 'Open task B inbox' }));
  await act(async () => { resolveSearch([{ memory: { id: 'A', status: 'proposed', proposal: { title: 'old-A-result', kind: 'discovery', scope: 'task', confidence: 1, evidence: [] } } }]); });
  expect(screen.getByLabelText('Task scope')).toHaveValue('B');
  expect(screen.queryByText(/old-A-result|context-A-evidence/)).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Search memory inbox' })).toBeEnabled();
});
