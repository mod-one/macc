import { forwardRef, useImperativeHandle, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';

export interface VirtualizedLogLinesHandle {
  scrollToLine: (index: number) => void;
  scrollToBottom: () => void;
}

interface VirtualizedLogLinesProps {
  lines: string[];
  offset: number;
  highlightedIndex: number | null;
}

export const VirtualizedLogLines = forwardRef<VirtualizedLogLinesHandle, VirtualizedLogLinesProps>(
  function VirtualizedLogLines({ lines, offset, highlightedIndex }, ref) {
    const shouldVirtualize = lines.length > 200;
    const parentRef = useRef<HTMLDivElement>(null);
    const scrollToLineInStaticMode = (index: number) => {
      const parent = parentRef.current;
      if (!parent) return;
      const lineEl = parent.querySelector<HTMLElement>(`[data-line-index="${index}"]`);
      if (lineEl && typeof lineEl.scrollIntoView === 'function') {
        lineEl.scrollIntoView({ block: 'center' });
        return;
      }
      // Fallback to estimated row height when the target line is not found.
      parent.scrollTop = Math.max(0, index * 24);
    };

    const rowVirtualizer = useVirtualizer({
      count: shouldVirtualize ? lines.length : 0,
      getScrollElement: () => parentRef.current,
      estimateSize: () => 24,
      overscan: 20,
      initialRect: { width: 0, height: 512 },
    });

    useImperativeHandle(
      ref,
      () => ({
        scrollToLine: (index: number) => {
          if (index < 0 || index >= lines.length) return;
          if (!shouldVirtualize) {
            scrollToLineInStaticMode(index);
            return;
          }
          rowVirtualizer.scrollToIndex(index, { align: 'center' });
        },
        scrollToBottom: () => {
          if (lines.length === 0) return;
          if (!shouldVirtualize) {
            const parent = parentRef.current;
            if (!parent) return;
            parent.scrollTop = parent.scrollHeight;
            return;
          }
          rowVirtualizer.scrollToIndex(lines.length - 1, { align: 'end' });
        },
      }),
      [lines.length, rowVirtualizer, shouldVirtualize],
    );

    return (
      <div
        ref={parentRef}
        className="flex-1 overflow-auto rounded-2xl border border-slate-200 bg-slate-950 p-0 font-mono text-xs leading-6 text-slate-200"
        style={{ height: '32rem' }}
      >
        {!shouldVirtualize ? (
          <div>
            {lines.map((line, index) => {
              const lineNum = offset + index + 1;
              const isHighlighted = highlightedIndex === index;
              return (
                <div
                  key={lineNum}
                  data-line-index={index}
                  className={`flex border-b border-slate-900/60 transition-colors ${
                    isHighlighted ? 'bg-amber-900/40' : 'hover:bg-slate-800/50'
                  }`}
                >
                  <div className="select-none border-r border-slate-800 px-3 py-0 text-right text-slate-600">
                    {lineNum}
                  </div>
                  <div className="min-w-0 flex-1 whitespace-pre-wrap break-all px-3 py-0">{line}</div>
                </div>
              );
            })}
          </div>
        ) : (
          <div style={{ height: `${rowVirtualizer.getTotalSize()}px`, position: 'relative' }}>
            {rowVirtualizer.getVirtualItems().map((virtualRow) => {
              const line = lines[virtualRow.index] ?? '';
              const lineNum = offset + virtualRow.index + 1;
              const isHighlighted = highlightedIndex === virtualRow.index;
              return (
                <div
                  key={lineNum}
                  data-index={virtualRow.index}
                  data-line-index={virtualRow.index}
                  ref={rowVirtualizer.measureElement}
                  className={`absolute left-0 top-0 flex w-full border-b border-slate-900/60 transition-colors ${
                    isHighlighted ? 'bg-amber-900/40' : 'hover:bg-slate-800/50'
                  }`}
                  style={{ transform: `translateY(${virtualRow.start}px)` }}
                >
                  <div className="select-none border-r border-slate-800 px-3 py-0 text-right text-slate-600">
                    {lineNum}
                  </div>
                  <div className="min-w-0 flex-1 whitespace-pre-wrap break-all px-3 py-0">
                    {line}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    );
  },
);
