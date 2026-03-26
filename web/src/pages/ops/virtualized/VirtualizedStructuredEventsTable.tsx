import { useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';

export interface StructuredEventRecord {
  line: number;
  raw: string;
  parsed: Record<string, unknown> | null;
}

interface VirtualizedStructuredEventsTableProps {
  events: StructuredEventRecord[];
  formatTimestamp: (value: string) => string;
}

export function VirtualizedStructuredEventsTable({
  events,
  formatTimestamp,
}: VirtualizedStructuredEventsTableProps) {
  const shouldVirtualize = events.length > 200;
  const parentRef = useRef<HTMLDivElement>(null);
  const rowVirtualizer = useVirtualizer({
    count: shouldVirtualize ? events.length : 0,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 34,
    overscan: 16,
    initialRect: { width: 0, height: 576 },
  });

  return (
    <div
      ref={parentRef}
      className="overflow-auto rounded-2xl border border-slate-200 bg-white shadow-sm"
      style={{ height: '36rem' }}
    >
      <table className="w-full border-collapse text-xs">
        <thead className="sticky top-0 z-10 bg-slate-50">
          <tr>
            <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
              #
            </th>
            <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
              Timestamp
            </th>
            <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
              Type
            </th>
            <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
              Status
            </th>
            <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
              Source
            </th>
            <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
              Task
            </th>
            <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
              Phase
            </th>
            <th className="border-b border-slate-200 px-3 py-2 text-left font-semibold uppercase tracking-[0.12em] text-slate-500">
              Details
            </th>
          </tr>
        </thead>
        {!shouldVirtualize ? (
          <tbody>
            {events.map((ev) => {
              if (!ev.parsed) {
                return (
                  <tr key={ev.line} className="border-b border-slate-100 hover:bg-slate-50">
                    <td className="px-3 py-1.5 text-slate-400">{ev.line}</td>
                    <td colSpan={7} className="max-w-md truncate px-3 py-1.5 font-mono text-slate-500">
                      {ev.raw}
                    </td>
                  </tr>
                );
              }

              const p = ev.parsed;
              return (
                <tr key={ev.line} className="border-b border-slate-100 hover:bg-slate-50">
                  <td className="px-3 py-1.5 text-slate-400">{ev.line}</td>
                  <td className="whitespace-nowrap px-3 py-1.5 text-slate-600">
                    {typeof p.ts === 'string' ? formatTimestamp(p.ts) : '-'}
                  </td>
                  <td className="px-3 py-1.5">
                    <span className="inline-block rounded-full bg-sky-100 px-2 py-0.5 text-[10px] font-semibold uppercase text-sky-800">
                      {String(p.type ?? '-')}
                    </span>
                  </td>
                  <td className="px-3 py-1.5 text-slate-700">{String(p.status ?? '-')}</td>
                  <td className="px-3 py-1.5 text-slate-600">{String(p.source ?? '-')}</td>
                  <td className="px-3 py-1.5 font-mono text-slate-600">{String(p.task_id ?? '-')}</td>
                  <td className="px-3 py-1.5 text-slate-600">{String(p.phase ?? '-')}</td>
                  <td className="max-w-xs truncate px-3 py-1.5 text-slate-500" title={ev.raw}>
                    {typeof p.msg === 'string' ? p.msg : typeof p.detail === 'string' ? p.detail : '-'}
                  </td>
                </tr>
              );
            })}
          </tbody>
        ) : (
          <tbody style={{ height: `${rowVirtualizer.getTotalSize()}px`, position: 'relative' }}>
            {rowVirtualizer.getVirtualItems().map((virtualRow) => {
              const ev = events[virtualRow.index];
              if (!ev) return null;
              if (!ev.parsed) {
                return (
                  <tr
                    key={ev.line}
                    data-index={virtualRow.index}
                    ref={rowVirtualizer.measureElement}
                    className="absolute left-0 top-0 w-full border-b border-slate-100 hover:bg-slate-50"
                    style={{ transform: `translateY(${virtualRow.start}px)` }}
                  >
                    <td className="px-3 py-1.5 text-slate-400">{ev.line}</td>
                    <td colSpan={7} className="max-w-md truncate px-3 py-1.5 font-mono text-slate-500">
                      {ev.raw}
                    </td>
                  </tr>
                );
              }
  
              const p = ev.parsed;
              return (
                <tr
                  key={ev.line}
                  data-index={virtualRow.index}
                  ref={rowVirtualizer.measureElement}
                  className="absolute left-0 top-0 w-full border-b border-slate-100 hover:bg-slate-50"
                  style={{ transform: `translateY(${virtualRow.start}px)` }}
                >
                  <td className="px-3 py-1.5 text-slate-400">{ev.line}</td>
                  <td className="whitespace-nowrap px-3 py-1.5 text-slate-600">
                    {typeof p.ts === 'string' ? formatTimestamp(p.ts) : '-'}
                  </td>
                  <td className="px-3 py-1.5">
                    <span className="inline-block rounded-full bg-sky-100 px-2 py-0.5 text-[10px] font-semibold uppercase text-sky-800">
                      {String(p.type ?? '-')}
                    </span>
                  </td>
                  <td className="px-3 py-1.5 text-slate-700">{String(p.status ?? '-')}</td>
                  <td className="px-3 py-1.5 text-slate-600">{String(p.source ?? '-')}</td>
                  <td className="px-3 py-1.5 font-mono text-slate-600">{String(p.task_id ?? '-')}</td>
                  <td className="px-3 py-1.5 text-slate-600">{String(p.phase ?? '-')}</td>
                  <td className="max-w-xs truncate px-3 py-1.5 text-slate-500" title={ev.raw}>
                    {typeof p.msg === 'string' ? p.msg : typeof p.detail === 'string' ? p.detail : '-'}
                  </td>
                </tr>
              );
            })}
          </tbody>
        )}
      </table>
    </div>
  );
}
