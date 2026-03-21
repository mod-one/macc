import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter } from 'react-router-dom';
import type { ApiConfigResponse } from '../../api/models';
import Tools from './Tools';

const getConfigMock = vi.fn();
const updateConfigMock = vi.fn();

vi.mock('../../api/client', () => ({
  getConfig: (...args: unknown[]) => getConfigMock(...args),
  updateConfig: (...args: unknown[]) => updateConfigMock(...args),
  ApiClientError: class ApiClientError extends Error {
    envelope = {
      error: {
        code: 'MACC-WEB-0000',
        message: 'Mock error',
      },
    };
  },
}));

function buildConfig(): ApiConfigResponse {
  return {
    enabledTools: ['codex'],
    toolConfig: {
      codex: {
        version: '1.0.0',
        category: 'assistant',
        capabilities: ['edit', 'review'],
      },
    },
    toolSettings: {
      codex: {
        network: {
          enabled: true,
        },
      },
    },
    toolPriority: ['codex'],
  } as unknown as ApiConfigResponse;
}

function renderPage(): void {
  render(
    <MemoryRouter>
      <Tools />
    </MemoryRouter>,
  );
}

describe('Tools page', () => {
  beforeEach(() => {
    getConfigMock.mockReset();
    updateConfigMock.mockReset();
  });

  it('keeps raw JSON editor text while invalid JSON is being typed', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    renderPage();

    await screen.findByText('Tools & Adapters');
    fireEvent.click(screen.getByRole('heading', { name: 'Codex' }));
    fireEvent.click(screen.getByRole('button', { name: 'Raw JSON' }));

    const rawEditor = screen.getByLabelText('Raw JSON editor') as HTMLTextAreaElement;
    fireEvent.change(rawEditor, { target: { value: '{' } });

    expect(rawEditor.value).toBe('{');
    expect(screen.getByText('Invalid JSON. Fix before applying.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Apply Changes' })).toBeDisabled();
  });

  it('asks for confirmation before check updates when unsaved changes exist', async () => {
    getConfigMock.mockResolvedValue(buildConfig());
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(false);
    renderPage();

    await screen.findByText('Tools & Adapters');
    fireEvent.click(screen.getByRole('checkbox', { name: /enabled/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Check Updates' }));

    expect(confirmSpy).toHaveBeenCalledTimes(1);
    expect(getConfigMock).toHaveBeenCalledTimes(1);
    confirmSpy.mockRestore();
  });
});
