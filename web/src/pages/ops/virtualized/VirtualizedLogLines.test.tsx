import { createRef } from 'react';
import { act, render } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  VirtualizedLogLines,
  type VirtualizedLogLinesHandle,
} from './VirtualizedLogLines';

describe('VirtualizedLogLines', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('scrollToLine works for non-virtualized content', () => {
    const ref = createRef<VirtualizedLogLinesHandle>();
    const { container } = render(
      <VirtualizedLogLines
        ref={ref}
        lines={['one', 'two', 'three']}
        offset={0}
        highlightedIndex={null}
      />,
    );
    const scrollContainer = container.firstElementChild as HTMLDivElement;
    scrollContainer.scrollTop = 0;

    act(() => {
      ref.current?.scrollToLine(1);
    });

    expect(scrollContainer.scrollTop).toBe(24);
  });

  it('scrollToBottom works for non-virtualized content', () => {
    const ref = createRef<VirtualizedLogLinesHandle>();
    const { container } = render(
      <VirtualizedLogLines
        ref={ref}
        lines={['one', 'two', 'three']}
        offset={0}
        highlightedIndex={null}
      />,
    );

    const scrollContainer = container.firstElementChild as HTMLDivElement;
    Object.defineProperty(scrollContainer, 'scrollHeight', {
      configurable: true,
      value: 500,
    });
    scrollContainer.scrollTop = 0;

    act(() => {
      ref.current?.scrollToBottom();
    });

    expect(scrollContainer.scrollTop).toBe(500);
  });
});
