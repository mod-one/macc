import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { KpiCard } from './KpiCard';

describe('KpiCard', () => {
  it('renders the title and value', () => {
    render(<KpiCard title="Healthy workers" value="18" />);

    expect(screen.getByText('Healthy workers')).toBeInTheDocument();
    expect(screen.getByText('18')).toBeInTheDocument();
  });
});
