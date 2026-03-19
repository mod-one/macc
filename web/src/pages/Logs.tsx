import React from 'react';
import type { ApiEventStreamMessage } from '../api/models';
import { useEventSource } from '../hooks/useEventSource';

const EVENT_LIMIT = 250;

function connectionTone(state: 'connecting' | 'open' | 'closed'): string {
  if (state === 'open') {
    return 'border-emerald-200 bg-emerald-50 text-emerald-900';
  }
  if (state === 'closed') {
    return 'border-rose-200 bg-rose-50 text-rose-900';
  }
  return 'border-amber-200 bg-amber-50 text-amber-900';
}

function formatTimestamp(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toISOString().replace('T', ' ').replace('.000Z', 'Z');
}

function summarizeEvent(event: ApiEventStreamMessage): string {
  const { payload } = event;
  const fragments = [payload.msg, payload.detail, payload.command, payload.event, payload.state];
  const summary = fragments.find((fragment) => typeof fragment === 'string' && fragment.length > 0);
  return summary ?? 'Coordinator event received.';
}

function eventDetails(event: ApiEventStreamMessage): string | null {
  const details: Record<string, unknown> = {};
  for (const key of ['task_id', 'phase', 'status', 'source', 'event_id', 'seq'] as const) {
    const value = event.payload[key];
    if (value !== undefined) {
      details[key] = value;
    }
  }
  if (Object.keys(details).length === 0 && event.payload.payload === undefined) {
    return null;
  }
  return JSON.stringify(
    event.payload.payload === undefined ? details : { ...details, payload: event.payload.payload },
    null,
    2,
  );
}

const Logs: React.FC = () => {
  const { connectionState, events } = useEventSource('/events', { maxEvents: EVENT_LIMIT });
  const [selectedType, setSelectedType] = React.useState<string>('all');

  const eventTypes = Array.from(new Set(events.map((event) => event.payload.type))).sort();
  const filteredEvents =
    selectedType === 'all'
      ? events
      : events.filter((event) => event.payload.type === selectedType);

  return (
    <section className="mx-auto flex w-full max-w-6xl flex-col gap-6 text-slate-700">
      <header className="rounded-[2rem] border border-slate-200 bg-[radial-gradient(circle_at_top_left,_rgba(56,189,248,0.16),_transparent_38%),linear-gradient(135deg,#ffffff,#f8fafc)] p-6 shadow-sm">
        <div className="flex flex-col gap-5 lg:flex-row lg:items-end lg:justify-between">
          <div className="max-w-2xl">
            <p className="text-sm font-semibold uppercase tracking-[0.2em] text-sky-700">
              Coordinator stream
            </p>
            <h1 className="mb-3 mt-2 text-5xl font-semibold tracking-tight text-slate-950">Logs</h1>
            <p className="max-w-xl text-base leading-7 text-slate-600">
              Live coordinator events from the server-sent events stream. The feed keeps the most
              recent {EVENT_LIMIT} records in memory so the page stays responsive under load.
            </p>
          </div>

          <div className="grid gap-3 rounded-2xl border border-slate-200 bg-white/90 p-4 text-sm text-slate-600 shadow-sm sm:grid-cols-3">
            <div>
              <p className="font-medium uppercase tracking-[0.16em] text-slate-500">Connection</p>
              <div
                className={`mt-2 inline-flex rounded-full border px-3 py-1 text-sm font-semibold capitalize ${connectionTone(connectionState)}`}
              >
                {connectionState}
              </div>
            </div>
            <div>
              <p className="font-medium uppercase tracking-[0.16em] text-slate-500">Buffered</p>
              <p className="mt-2 text-2xl font-semibold text-slate-950">{events.length}</p>
            </div>
            <div>
              <label
                className="font-medium uppercase tracking-[0.16em] text-slate-500"
                htmlFor="log-type-filter"
              >
                Event type
              </label>
              <select
                id="log-type-filter"
                className="mt-2 w-full rounded-xl border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 shadow-sm outline-none transition focus:border-sky-500"
                value={selectedType}
                onChange={(event) => {
                  setSelectedType(event.target.value);
                }}
              >
                <option value="all">All events</option>
                {eventTypes.map((eventType) => (
                  <option key={eventType} value={eventType}>
                    {eventType}
                  </option>
                ))}
              </select>
            </div>
          </div>
        </div>
      </header>

      <section className="rounded-[2rem] border border-slate-200 bg-white p-4 shadow-sm">
        <div className="mb-4 flex items-center justify-between gap-3 px-2">
          <div>
            <h2 className="text-2xl font-semibold text-slate-950">Live feed</h2>
            <p className="text-sm leading-6 text-slate-500">
              Heartbeats and coordinator events arrive over the same stream.
            </p>
          </div>
          <p className="text-sm text-slate-500">
            Showing {filteredEvents.length} of {events.length}
          </p>
        </div>

        <div className="h-[32rem] overflow-y-auto rounded-[1.5rem] border border-slate-200 bg-slate-950/95 p-3">
          {filteredEvents.length === 0 ? (
            <div className="flex h-full items-center justify-center rounded-[1.2rem] border border-dashed border-slate-700 bg-slate-950 px-6 text-center text-sm text-slate-400">
              {events.length === 0
                ? 'Waiting for the first event from /api/v1/events.'
                : 'No events match the selected type.'}
            </div>
          ) : (
            <div className="flex flex-col gap-3">
              {filteredEvents.map((event) => {
                const details = eventDetails(event);
                return (
                  <article
                    key={`${event.payload.event_id}-${event.stream}-${event.receivedAt}`}
                    className="rounded-[1.2rem] border border-slate-800 bg-slate-900 p-4 text-slate-100 shadow-sm"
                  >
                    <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                      <div>
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="rounded-full bg-sky-500/15 px-2.5 py-1 text-xs font-semibold uppercase tracking-[0.14em] text-sky-200">
                            {event.payload.type}
                          </span>
                          <span className="rounded-full bg-slate-800 px-2.5 py-1 text-xs font-medium uppercase tracking-[0.14em] text-slate-300">
                            {event.stream}
                          </span>
                          <span className="rounded-full bg-slate-800 px-2.5 py-1 text-xs font-medium uppercase tracking-[0.14em] text-slate-300">
                            {String(event.payload.status)}
                          </span>
                        </div>
                        <p className="mt-3 text-sm leading-6 text-slate-200">{summarizeEvent(event)}</p>
                      </div>

                      <div className="text-xs uppercase tracking-[0.16em] text-slate-400 lg:text-right">
                        <p>{formatTimestamp(event.payload.ts)}</p>
                        <p className="mt-1">seq {event.payload.seq}</p>
                      </div>
                    </div>

                    <div className="mt-4 grid gap-2 text-sm text-slate-300 sm:grid-cols-2 xl:grid-cols-4">
                      <p>
                        <span className="text-slate-500">Source:</span> {event.payload.source}
                      </p>
                      <p>
                        <span className="text-slate-500">Task:</span>{' '}
                        {typeof event.payload.task_id === 'string' ? event.payload.task_id : 'n/a'}
                      </p>
                      <p>
                        <span className="text-slate-500">Phase:</span>{' '}
                        {typeof event.payload.phase === 'string' ? event.payload.phase : 'n/a'}
                      </p>
                      <p>
                        <span className="text-slate-500">Event ID:</span> {event.payload.event_id}
                      </p>
                    </div>

                    {details ? (
                      <pre className="mt-4 overflow-x-auto rounded-xl border border-slate-800 bg-slate-950 px-3 py-3 text-xs leading-6 text-slate-300">
                        {details}
                      </pre>
                    ) : null}
                  </article>
                );
              })}
            </div>
          )}
        </div>
      </section>
    </section>
  );
};

export default Logs;
