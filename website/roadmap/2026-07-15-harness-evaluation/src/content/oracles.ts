/* oracles — the eight independent evidence sources (integration). */

export const ORACLES = [
  {
    name: 'send response',
    proves: 'acceptance, identity, steering, queueing, and deduplication flags; absence normalizes to false.',
  },
  {
    name: 'full transcript',
    proves: 'durable order and content, function results, and the absence of duplicate entries; usage only when the shipped transcript persists it.',
  },
  {
    name: 'status',
    proves: 'terminal state, result, pending calls, live children, queue, and retry counters.',
  },
  {
    name: 'target recorder',
    proves: 'invocation arguments, count, order, and forbidden side effects, durably appended before the handler responds.',
  },
  {
    name: 'scripted router',
    proves: 'generation count and the exact model-visible messages and tools.',
  },
  {
    name: 'lifecycle recorder',
    proves: 'completed delivery and duplicate consistency across at-least-once fan-out; v1 records only harness::turn-completed.',
  },
  {
    name: 'process supervisor',
    proves: 'unexpected exit, restart boundaries, and shutdown behavior.',
  },
  {
    name: 'traces / logs',
    proves: 'diagnosis only: never an ordinary oracle unless telemetry is the explicit invariant.',
    diagnostic: true,
  },
] as const
