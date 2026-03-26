import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import Help from './Help';

function renderHelp(initialPath = '/help') {
  return render(
    <MemoryRouter initialEntries={[initialPath]}>
      <Routes>
        <Route path="/help" element={<Help />} />
      </Routes>
    </MemoryRouter>,
  );
}

describe('Help page', () => {
  it('shows the core documentation sections', () => {
    renderHelp();

    expect(screen.getByRole('button', { name: 'Getting Started' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Web UI' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Commands' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Configuration' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Coordinator' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Troubleshooting' })).toBeInTheDocument();
  });

  it('uses the section query param when provided', () => {
    renderHelp('/help?section=commands');

    expect(screen.getByRole('heading', { level: 1, name: 'Commands' })).toBeInTheDocument();
    expect(screen.getByText(/Ctrl\+K/)).toBeInTheDocument();
  });

  it('filters sections with client-side search', () => {
    renderHelp();

    fireEvent.change(screen.getByRole('searchbox', { name: 'Search help docs' }), {
      target: { value: 'adapter-level' },
    });

    expect(screen.getByRole('button', { name: 'Configuration' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Getting Started' })).not.toBeInTheDocument();
  });

  it('shows an empty state when no section matches search', () => {
    renderHelp();

    fireEvent.change(screen.getByRole('searchbox', { name: 'Search help docs' }), {
      target: { value: 'zzzz-no-match' },
    });

    expect(screen.getByText('No sections match your search.')).toBeInTheDocument();
  });
});
