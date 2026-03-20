import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ConfirmDialog } from './ConfirmDialog';

describe('ConfirmDialog', () => {
  it('supports confirm and cancel actions', async () => {
    const user = userEvent.setup();
    const handleConfirm = vi.fn();
    const handleOpenChange = vi.fn();

    const { rerender } = render(
      <ConfirmDialog
        description="Delete the selected item."
        onConfirm={handleConfirm}
        onOpenChange={handleOpenChange}
        open
        title="Delete item"
      />,
    );

    await user.click(screen.getByRole('button', { name: 'Confirm' }));
    expect(handleConfirm).toHaveBeenCalledTimes(1);

    rerender(
      <ConfirmDialog
        description="Delete the selected item."
        onConfirm={handleConfirm}
        onOpenChange={handleOpenChange}
        open
        title="Delete item"
      />,
    );

    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(handleOpenChange).toHaveBeenCalledWith(false);
  });
});
