/**
 * The words around the charts on /benchmarks, written from the data: every number in a sentence comes from the
 * same run the charts draw, so the copy can never disagree with the picture. Runs at build time.
 */

import type { Figure } from '@/components/benchmarks/figures'
import type { MicroGroupView } from '@/components/benchmarks/micro'

import { bytes, compact, count, joined, mebibytes, micros, percent, rate } from './format'
import type { Highlights, MicroData, Summary } from './types'
import { microRows, type ProfileView, profiles } from './views'

const f = joined

/** The six figures of the summary grid, per machine size. */
export function figures(summary: Summary): Record<string, Figure[]> {
  return Object.fromEntries(
    profiles(summary).map((p) => {
      const h = summary.highlights[p.name]
      return [p.name, h ? figuresFor(h) : []]
    }),
  )
}

function figuresFor(h: Highlights): Figure[] {
  const perCore = h.cpu_us_per_call ? 1e6 / h.cpu_us_per_call : null
  return [
    {
      id: 'calls',
      label: 'Calls per second through the engine',
      value: h.calls_per_sec,
      format: 'perSec',
      detail: h.calls_per_sec
        ? `Worker to engine to worker and back, ${count(h.calls_concurrency ?? 0)} calls in flight. p99 ${f(micros(h.calls_p99_us ?? 0))}.`
        : 'Not measured on this size.',
    },
    {
      id: 'cpu',
      label: 'Engine CPU per call',
      value: h.cpu_us_per_call,
      format: 'micros',
      detail: perCore
        ? `About ${f(compact(perCore)).replace(' ', '')} calls a second for each core the engine has.`
        : 'Not measured on this size.',
    },
    {
      id: 'sustained',
      label: 'Observer activations per second, sustained',
      value: h.sustained_activations_per_sec,
      format: 'perSec',
      detail:
        h.sustained_activations_per_sec === null
          ? 'Not measured on this size.'
          : h.saturated_at
            ? `Every event delivered. Offer ${f(rate(h.saturated_at))} and ${percent(h.saturated_delivered ?? 0)} arrive; the rest is dropped, and counted.`
            : 'Every event delivered, up to the highest rate offered.',
    },
    {
      id: 'local',
      label: 'Calls a worker makes to its own functions',
      value: h.local_calls_per_sec,
      format: 'perSec',
      detail:
        h.local_calls_per_sec === null
          ? 'Not measured on this size.'
          : `In-process, unobserved: the engine spends ${(h.local_engine_cpu ?? 0).toFixed(2)} CPU on them.`,
    },
    {
      id: 'worker',
      label: 'Engine memory per connected worker',
      value: h.bytes_per_worker,
      format: 'bytes',
      detail:
        h.bytes_per_worker && h.workers_held
          ? `${count(h.workers_held)} idle workers held: ${f(bytes(h.bytes_per_worker * h.workers_held))} on top of ${f(mebibytes(h.idle_rss_mb ?? 0))} at rest.`
          : 'Not measured on this size.',
    },
    {
      id: 'observer',
      label: 'Engine memory per observer',
      value: h.bytes_per_observer,
      format: 'bytes',
      detail:
        h.bytes_per_observer && h.observers_held
          ? `${count(h.observers_held)} observers held: ${f(bytes(h.bytes_per_observer * h.observers_held))}.${stoppedBy(h, 'observers')}`
          : 'Not measured on this size.',
    },
  ]
}

function stoppedBy(h: Highlights, scenario: string): string {
  const failure = h.failed.find((x) => x.scenario === scenario)
  return failure?.oom_killed ? ' The next step ran out of memory.' : ''
}

/** "1 vCPU 31k/s, 2 vCPU 72k/s, Host 480k/s": one number across sizes, smallest first. */
export function acrossSizes(
  summary: Summary,
  pick: (h: Highlights) => number | null,
  format: (v: number) => string,
): { profile: ProfileView; text: string }[] {
  return profiles(summary).flatMap((p) => {
    const v = pick(summary.highlights[p.name] ?? ({} as Highlights))
    return v === null || v === undefined ? [] : [{ profile: p, text: format(v) }]
  })
}

/** Idle workers (or observers) one size's memory cap leaves room for, at the measured cost of each. */
export function fitsIn(p: ProfileView, h: Highlights | undefined, per: number | null): number | null {
  if (!p.mem_mb || !h || !per || h.idle_rss_mb === null) return null
  return Math.max(0, ((p.mem_mb - h.idle_rss_mb) * 1024 * 1024) / per)
}

/** The micro-benchmark groups the page shows open, in order, with what each one times. */
const featured: { id: string; title: string; description: string }[] = [
  {
    id: 'ws_roundtrip',
    title: 'WebSocket round trip',
    description: 'One call from a worker, through the engine, to another worker and back, over real sockets.',
  },
  {
    id: 'observe_dispatch',
    title: 'Observing a call',
    description:
      'A call with N observers on it: default delivery reaches every one, filtered evaluates every filter and matches one. Fan-out is inside the time.',
  },
  {
    id: 'concurrent_invocation',
    title: 'Concurrent calls',
    description: 'N calls at once to a function hosted in the engine, waiting for all of them.',
  },
  {
    id: 'invoke_function_payload_sizes',
    title: 'Payload size',
    description: 'One call carrying 1 KB to 1 MB of JSON.',
  },
  {
    id: 'trigger_fanout',
    title: 'Observer fan-out',
    description: 'One call delivered to N observers at once.',
  },
  {
    id: 'control_plane_churn',
    title: 'Registration churn',
    description: 'Registering and removing functions, workers and observers by the hundred and the thousand.',
  },
]

const descriptions: Record<string, [string, string]> = {
  core_runtime: ['Core runtime', 'Calling a handler registered in the engine.'],
  kv_store: ['Key-value store', 'Reads, writes, updates and listings in the built-in store.'],
  kv_store_contention: ['Key-value contention', 'N writers on the same key at once.'],
  protocol_serialize: ['Protocol: encode', 'Turning each wire message into JSON.'],
  protocol_deserialize: ['Protocol: decode', 'Parsing each wire message from JSON.'],
  protocol_roundtrip: ['Protocol: round trip', 'Encoding and decoding each wire message.'],
  pubsub_fanout: ['Pub/sub fan-out', 'One message to N subscribers.'],
  queue_enqueue: ['Queue: enqueue', 'Putting one job on a queue.'],
  queue_enqueue_concurrent: ['Queue: concurrent producers', 'N producers enqueueing at once.'],
  queue_enqueue_payload_sizes: ['Queue: payload size', 'Enqueueing jobs of 1 KB to 1 MB.'],
  startup: ['Engine startup', 'From launch to accepting workers.'],
  worker_cleanup: ['Worker cleanup', 'Tearing down a worker that leaves with its functions and calls in flight.'],
  worker_invocation_tracking: ['Invocation tracking', 'Recording which worker each call belongs to.'],
  worker_invocation_tracking_concurrent: ['Invocation tracking, concurrent', 'The same with N calls at once.'],
  state_adapter: ['State', 'Reads and writes through the state adapter.'],
}

function titleOf(id: string): string {
  return id.replace(/_/g, ' ').replace(/^\w/, (c) => c.toUpperCase())
}

/** The micro-benchmark groups: the featured ones, then the rest. Groups retired before the latest release are
 * left out (their rows are all gone). */
export function microGroups(micro: MicroData): { featured: MicroGroupView[]; more: MicroGroupView[] } {
  const view = (id: string, title: string, description: string): MicroGroupView | null => {
    const rows = microRows(micro, id)
    return rows.length ? { id, title, description, rows } : null
  }
  const shown = featured.flatMap((g) => view(g.id, g.title, g.description) ?? [])
  const shownIds = new Set(shown.map((g) => g.id))
  const more = micro.groups
    .filter((g) => !shownIds.has(g.id))
    .flatMap((g) => {
      const [title, description] = descriptions[g.id] ?? [titleOf(g.id), '']
      return view(g.id, title, description) ?? []
    })
  return { featured: shown, more }
}

/** The profile the page opens on: the 2 vCPU VM when there is one (a common small production box). */
export function initialProfile(summary: Summary): string {
  const all = profiles(summary)
  return (all.find((p) => p.kind === 'vm' && p.cpus === 2) ?? all.find((p) => p.kind === 'vm') ?? all[0])?.name ?? ''
}

export { bytes, count, f, mebibytes, micros, percent, rate }
