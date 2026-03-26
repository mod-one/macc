import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';
import GitGraphPanel from './GitGraphPanel';

vi.mock('./GitGraphView', () => ({
  default: () => null,
}));

describe('GitGraphPanel', () => {
  it('supports keyboard resizing from the separator handle', () => {
    const storage = {
      getItem: vi.fn(() => null),
      setItem: vi.fn(),
    };
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      value: storage,
    });

    const { container } = render(
      <MemoryRouter initialEntries={['/dashboard']}>
        <GitGraphPanel />
      </MemoryRouter>,
    );

    const separator = screen.getByRole('separator', { name: /resize git graph panel/i });
    const panel = container.querySelector('aside');

    expect(panel).not.toBeNull();
    expect(panel).toHaveStyle({ width: '350px' });

    separator.focus();
    fireEvent.keyDown(separator, { key: 'ArrowRight' });

    expect(panel).toHaveStyle({ width: '374px' });
  });
});
