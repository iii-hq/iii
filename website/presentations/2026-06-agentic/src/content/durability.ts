/**
 * durability.ts — the stage data DurableSection feeds the shared
 * DurabilityTimeline archetype: one turn surviving a crash and a long wait.
 */

import type { TimelineStage } from '@lib/components/diagrams/DurabilityTimeline'

export const DURABILITY_STAGES: TimelineStage[] = [
  {
    id: 'send',
    label: 'harness::send',
    sub: 'turn t_001 seeded',
    tone: 'ink',
    title: 'a turn becomes a record, not a process',
    desc: 'send persists the message, seeds the turn record with an atomic check-and-set, enqueues step 1, and returns. from here on, the turn lives as durable state plus queue entries — never as a thread someone has to keep alive.',
    record: {
      status: 'running',
      step: '1 (enqueued)',
      calls: '{}',
      note: 'options frozen on the record for the whole turn',
    },
  },
  {
    id: 'gen',
    label: 'step 1 · generate',
    sub: 'e_t001_1_assistant',
    tone: 'ink',
    title: 'streaming into a deterministic entry',
    desc: 'the generate step appends the assistant entry under a deterministic id derived from turn + step, then streams deltas into it. the id is the durability trick: any replay of this step writes into the same entry.',
    record: {
      status: 'running',
      step: '1',
      calls: '{}',
      note: 'stream_request_id recorded — a stop can abort mid-flight',
    },
  },
  {
    id: 'crash',
    label: 'worker crashes',
    sub: 'mid-stream',
    tone: 'alert',
    title: 'the process dies. the turn does not.',
    desc: 'the queue delivery times out and redelivers; the turn record, the entries, and the per-call checkpoints are all still there. there is no in-memory loop to lose — that is the whole point.',
    record: {
      status: 'running',
      step: '1 (redelivering)',
      calls: '{}',
      note: 'partial assistant text already persisted + already rendered live',
    },
  },
  {
    id: 'resume',
    label: 'step 1 · redelivered',
    sub: 'same entry — no duplicate',
    tone: 'accent',
    title: 'resume, not restart',
    desc: 'the redelivered step finds the deterministic assistant entry already exists and streams into it instead of appending a second one. a crash never yields two answers — consumers just see the stream continue.',
    record: {
      status: 'running',
      step: '1',
      calls: '{}',
      note: "stale-step guard drops anything older than the record's step",
    },
  },
  {
    id: 'dispatch',
    label: 'step 2 · dispatch',
    sub: 'checkpoint per call',
    tone: 'ink',
    title: 'side effects are at-most-once',
    desc: 'each function call checkpoints dispatched before it runs and done after its result lands. a call caught mid-flight by a crash is never silently re-invoked — the model gets an explicit "result unknown" and decides whether to retry.',
    record: {
      status: 'awaiting_functions',
      step: '2',
      calls: '{ call_1: "done" }',
      note: 'function_result ids derive from the call id — replays are no-ops',
    },
  },
  {
    id: 'hold',
    label: 'turn parks',
    sub: 'held — hours pass',
    tone: 'warn',
    title: 'durable execution: waiting costs nothing',
    desc: 'a held call (an approval, a long-running child) parks the turn: the step ends, no queue entry stays open, nothing blocks. the turn sleeps as state — for minutes or for a week — while the rest of the system runs at full speed.',
    record: {
      status: 'awaiting_functions',
      step: '2 (parked)',
      calls: '{ call_2: "pending" }',
      note: 'pending timeout sweeps guarantee nothing parks forever',
    },
    gapAfter: true,
  },
  {
    id: 'resolve',
    label: 'function::resolve',
    sub: 'decision lands',
    tone: 'accent',
    title: 'one call wakes the turn',
    desc: 'whoever owns the decision — a human, a finished child, a timeout sweep — resolves the pending call. the harness appends the result under its deterministic id and re-enqueues the turn exactly where it parked.',
    record: {
      status: 'running',
      step: '3 (enqueued)',
      calls: '{ call_2: "done" }',
      note: 'duplicate resolves hit the same entry id — idempotent',
    },
  },
  {
    id: 'done',
    label: 'completed',
    sub: 'result + event',
    tone: 'ink',
    title: 'the turn ends with a typed result',
    desc: 'the turn record carries the output-contract result, the session flips to done, and turn_completed fires for everything that reacts to outcomes. total process restarts survived: one. messages lost: zero.',
    record: {
      status: 'completed',
      step: '3 (final)',
      calls: 'all done',
      note: "result validated against the contract's schema when supplied",
    },
  },
]
