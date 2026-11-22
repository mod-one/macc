import { useCoordinatorStore } from '../store';
import type { ApiCoordinatorStatus } from '../api/models';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import Layout from './Layout';

vi.mock('./GitGraphPanel', () => ({
  default: () => null,
}));

vi.mock('./GlobalSearch', () => ({
  default: () => null,
}));

vi.mock('../hooks/useNotificationCenter', () => ({
  useNotificationCenter: vi.fn(),
}));

function renderLayout(initialPath: string) {
  return render(
    <MemoryRouter initialEntries={[initialPath]}>
      <Routes>
        <Route path="/" element={<Layout />}>
          <Route path="*" element={<div>Route content</div>} />
        </Route>
      </Routes>
    </MemoryRouter>,
  );
}

describe('Layout contextual help button', () => {
  it.each([
    ['/config/tools', '/help?section=configuration'],
    ['/ops/logs', '/help?section=troubleshooting'],
    ['/plan', '/help?section=commands'],
  ] as const)('maps %s to %s', (initialPath, expectedHref) => {
    renderLayout(initialPath);

    expect(screen.getByRole('link', { name: 'Open contextual help' })).toHaveAttribute('href', expectedHref);
  });
});

it('links the Inbox badge to pending approvals even when every prompt is silent', () => {
  const previous = useCoordinatorStore.getState().status;
  useCoordinatorStore.setState({ status: { pending_approvals: [
    { task_id: 'A', awaiting_decision: true, auto_prompt: false },
    { task_id: 'B', awaiting_decision: true, auto_prompt: false },
    { task_id: 'C', awaiting_decision: false, auto_prompt: false },
  ] } as ApiCoordinatorStatus });
  const { unmount } = renderLayout('/dashboard');
  try {
    const link = screen.getByRole('link', { name: 'Inbox, 2 pending requests' });
    expect(link).toHaveAttribute('href', '/ops/inbox');
    expect(link).toHaveTextContent('▲ 2');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  } finally {
    unmount();
    useCoordinatorStore.setState({ status: previous });
  }
});
