import { render, screen, waitFor } from '@testing-library/react';
import axe from 'axe-core';
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

async function runAxe() {
  return axe.run(document.body, {
    rules: {
      'color-contrast': { enabled: false },
    },
  });
}

describe('Layout accessibility', () => {
  it.each(['/welcome', '/dashboard', '/ops/console', '/ops/logs'])(
    'has no critical axe violations at %s',
    async (initialPath) => {
      renderLayout(initialPath);

      const results = await runAxe();

      expect(results.violations).toEqual([]);
    },
  );

  it('exposes a skip link and moves focus to main content on navigation', async () => {
    renderLayout('/welcome');

    expect(screen.getByRole('link', { name: /skip to content/i })).toHaveAttribute(
      'href',
      '#main-content',
    );

    await waitFor(() => {
      expect(screen.getByRole('main')).toHaveFocus();
    });

    // Trigger a route change through keyboard-accessible navigation.
    screen.getByRole('link', { name: 'Dashboard' }).click();

    await waitFor(() => {
      expect(screen.getByRole('main')).toHaveFocus();
    });
  });
});
