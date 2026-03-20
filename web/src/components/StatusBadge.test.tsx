import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { StatusBadge } from './StatusBadge';

describe('StatusBadge', () => {
  it('renders the supplied label with the tone styling', () => {
    render(<StatusBadge status="Blocked" tone="blocked" />);

    const badge = screen.getByText('Blocked').closest('span');

    expect(badge).toBeInTheDocument();
    expect(badge).toHaveStyle({ color: 'var(--status-blocked)' });
  });
});
