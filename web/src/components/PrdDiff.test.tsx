import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import type { ApiPrdTask, JsonValue } from '../api/models';

const mockGetPrd = vi.fn();
vi.mock('../api/client', () => ({
  getPrd: (...args: unknown[]) => mockGetPrd(...args),
}));

// Dynamic import after mock setup
const { default: PrdDiff } = await import('./PrdDiff');

function makeTask(overrides: Partial<ApiPrdTask> = {}): ApiPrdTask {
  return {
    id: 'TASK-001',
    title: 'Original task',
    priority: '1',
    category: 'backend',
    scope: null,
    baseBranch: null,
    coordinatorTool: null,
    description: 'Original description',
    objective: null,
    result: null,
    dependencies: [],
    exclusiveResources: [],
    steps: ['step one'],
    notes: null,
    metadata: {},
    ...overrides,
  };
}

const savedTasks: ApiPrdTask[] = [makeTask()];
const savedMetadata: Record<string, JsonValue> = { version: '1.0' };
const modifiedTasks: ApiPrdTask[] = [makeTask({ title: 'Modified task', description: 'Updated description' })];

describe('PrdDiff', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetPrd.mockResolvedValue({ tasks: savedTasks, metadata: savedMetadata });
  });

  it('renders "No differences" when current matches saved', async () => {
    render(
      <PrdDiff
        currentTasks={savedTasks}
        currentMetadata={savedMetadata}
        hasUnsavedChanges={false}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('No differences found')).toBeInTheDocument();
    });
  });

  it('renders diff lines when current differs from saved', async () => {
    render(
      <PrdDiff
        currentTasks={modifiedTasks}
        currentMetadata={savedMetadata}
        hasUnsavedChanges={true}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText(/changes?/)).toBeInTheDocument();
    });
  });

  it('shows mode selector with both options', async () => {
    render(
      <PrdDiff
        currentTasks={savedTasks}
        currentMetadata={savedMetadata}
        hasUnsavedChanges={false}
      />,
    );

    const select = await waitFor(() => screen.getByRole('combobox'));
    expect(select).toBeInTheDocument();
    const options = select.querySelectorAll('option');
    expect(options).toHaveLength(2);
    expect(options[0].textContent).toBe('Unsaved vs Saved');
    expect(options[1].textContent).toBe('Project vs Worktree');
  });

  it('shows unified and split layout toggle buttons', async () => {
    render(
      <PrdDiff
        currentTasks={savedTasks}
        currentMetadata={savedMetadata}
        hasUnsavedChanges={false}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('Unified')).toBeInTheDocument();
      expect(screen.getByText('Split')).toBeInTheDocument();
    });
  });

  it('switches to split view on button click', async () => {
    render(
      <PrdDiff
        currentTasks={modifiedTasks}
        currentMetadata={savedMetadata}
        hasUnsavedChanges={true}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText(/changes?/)).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Split'));

    // Split view shows column headers
    await waitFor(() => {
      expect(screen.getByText('Saved')).toBeInTheDocument();
      expect(screen.getByText('Unsaved (editor)')).toBeInTheDocument();
    });
  });
});
