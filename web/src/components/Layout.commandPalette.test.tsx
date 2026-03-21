import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import Layout from './Layout';

vi.mock('./GitGraphPanel', () => ({
  default: () => null,
}));

function RouteProbe() {
  const location = useLocation();
  return <div data-testid="route-probe">{location.pathname}</div>;
}

function renderLayout(initialPath = '/welcome'): void {
  render(
    <MemoryRouter initialEntries={[initialPath]}>
      <Routes>
        <Route path="/" element={<Layout />}>
          <Route path="welcome" element={<div>Welcome Page</div>} />
          <Route path="dashboard" element={<div>Dashboard Page</div>} />
          <Route path="*" element={<RouteProbe />} />
        </Route>
      </Routes>
    </MemoryRouter>,
  );
}

describe('Layout command palette integration', () => {
  it('opens on Ctrl+K and closes on Escape', async () => {
    renderLayout();

    expect(screen.queryByPlaceholderText('Type a command or search routes...')).not.toBeInTheDocument();

    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });

    expect(await screen.findByPlaceholderText('Type a command or search routes...')).toBeInTheDocument();

    fireEvent.keyDown(document, { key: 'Escape' });

    await waitFor(() => {
      expect(screen.queryByPlaceholderText('Type a command or search routes...')).not.toBeInTheDocument();
    });
  });

  it('filters commands by typed query', async () => {
    renderLayout();

    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });

    const input = await screen.findByPlaceholderText('Type a command or search routes...');
    fireEvent.change(input, { target: { value: 'resume' } });

    expect(screen.getByRole('button', { name: /Resume Coordinator/i })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Go to Dashboard/i })).not.toBeInTheDocument();
  });

  it('executes selected command on Enter', async () => {
    renderLayout('/welcome');

    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });

    const input = await screen.findByPlaceholderText('Type a command or search routes...');
    fireEvent.change(input, { target: { value: 'dashboard' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(await screen.findByText('Dashboard Page')).toBeInTheDocument();
    expect(screen.queryByPlaceholderText('Type a command or search routes...')).not.toBeInTheDocument();
  });
});
