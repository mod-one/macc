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
