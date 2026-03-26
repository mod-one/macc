import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ConfirmDialog } from './ConfirmDialog';
import { ErrorBanner } from './ErrorBanner';
import { KpiCard } from './KpiCard';
import { RightDrawer } from './RightDrawer';
import { StatusBadge } from './StatusBadge';
import { TaskListItem } from './TaskListItem';
import { Toast } from './Toast';

describe('shared component library', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders KPI and status components with supplied content', () => {
    render(
      <>
        <KpiCard delta={12.4} title="Healthy workers" value="18" />
        <StatusBadge status="Active" tone="active" />
      </>,
    );

    expect(screen.getByText('Healthy workers')).toBeInTheDocument();
    expect(screen.getByText('+12.4%')).toBeInTheDocument();
    expect(screen.getByText('Active')).toBeInTheDocument();
  });

  it('supports keyboard dismissal on the right drawer', async () => {
    const handleOpenChange = vi.fn();

    render(
      <RightDrawer onOpenChange={handleOpenChange} open title="Inspector">
        Drawer body
      </RightDrawer>,
    );

    fireEvent.keyDown(document, { key: 'Escape' });

    await waitFor(() => expect(handleOpenChange).toHaveBeenCalledWith(false));
  });

  it('requires the typed phrase for dangerous confirmations', async () => {
    const handleConfirm = vi.fn();

    render(
      <ConfirmDialog
        confirmationPhrase="DELETE"
        description="Dangerous action"
        intent="danger"
        onConfirm={handleConfirm}
        onOpenChange={() => undefined}
        open
        title="Delete run"
      />,
    );

    const confirmButton = screen.getByRole('button', { name: 'Confirm' });
    expect(confirmButton).toBeDisabled();

    fireEvent.change(screen.getByLabelText(/type delete to continue/i), {
      target: { value: 'DELETE' },
    });

    expect(confirmButton).toBeEnabled();
    fireEvent.click(confirmButton);

    await waitFor(() => expect(handleConfirm).toHaveBeenCalledTimes(1));
  });

  it('supports a double-confirm flow for dangerous confirmations', async () => {
    const handleConfirm = vi.fn();

    render(
      <ConfirmDialog
        dangerousConfirmationMode="double-confirm"
        description="Dangerous action"
        intent="danger"
        onConfirm={handleConfirm}
        onOpenChange={() => undefined}
        open
        title="Delete run"
      />,
    );

    const confirmButton = screen.getByRole('button', { name: 'Confirm' });
    expect(confirmButton).toBeDisabled();

    fireEvent.click(screen.getByRole('checkbox'));

    expect(confirmButton).toBeEnabled();
    fireEvent.click(confirmButton);

    await waitFor(() => expect(handleConfirm).toHaveBeenCalledTimes(1));
  });

  it('wires error banner actions', () => {
    const onRetry = vi.fn();
    const onCopy = vi.fn();
    const onOpenLogs = vi.fn();

    render(
      <ErrorBanner
        code="MACC-WEB-0001"
        message="API failed"
        onCopy={onCopy}
        onOpenLogs={onOpenLogs}
        onRetry={onRetry}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: /retry/i }));
    fireEvent.click(screen.getByRole('button', { name: /copy/i }));
    fireEvent.click(screen.getByRole('button', { name: /open logs/i }));

    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(onCopy).toHaveBeenCalledTimes(1);
    expect(onOpenLogs).toHaveBeenCalledTimes(1);
  });

  it('renders a selectable task item', () => {
    const onSelect = vi.fn();

    render(
      <TaskListItem
        attempts={2}
        onSelect={onSelect}
        priority="P1"
        state="Blocked"
        stateTone="blocked"
        taskId="WEB2-UI-002"
        title="Build shared component library"
        tool="codex"
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: /build shared component library/i }));

    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it('dismisses toast notifications through the close action', async () => {
    const onOpenChange = vi.fn();

    render(
      <Toast
        description="Component library ready"
        onOpenChange={onOpenChange}
        open
        title="Saved"
        variant="success"
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: /close notification/i }));

    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
  });
});
