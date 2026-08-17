import type { SeqLane, SeqStep } from '@lib/components/diagrams/SequencePlayer'

export const RUN_LANES: SeqLane[] = [
  { id: 'launcher', label: 'launcher', x: 72 },
  { id: 'runner', label: 'runner', x: 232 },
  { id: 'harness', label: 'harness', x: 392 },
  { id: 'model', label: 'model boundary', x: 552 },
  { id: 'evidence', label: 'evidence', x: 712 },
  { id: 'report', label: 'report', x: 872 },
]

export const RUN_STEPS: SeqStep[] = [
  {
    from: 'launcher',
    to: 'runner',
    label: 'select track',
    title: 'choose the question before the run',
    desc: 'integration selects a fixture and engine; quality selects the complete corpus and one pinned subject.',
  },
  {
    from: 'runner',
    to: 'runner',
    label: 'resolve identity',
    title: 'make the run reproducible',
    desc: 'the runner records stack artifacts and either a scripted generation contract or model, prompt, skill, and worker identities.',
  },
  {
    from: 'runner',
    to: 'harness',
    label: 'boot dedicated stack',
    title: 'isolate the subject path',
    desc: 'the real harness and durable dependencies start against run-scoped state before any stimulus.',
  },
  {
    from: 'runner',
    to: 'harness',
    label: 'harness::send',
    title: 'enter through the shared public Function',
    desc: 'both tracks call the same synchronous SDK path; the harness still owns internal queueing and continuation.',
  },
  {
    from: 'harness',
    to: 'model',
    label: 'router::chat',
    title: 'cross the track-specific model boundary',
    desc: 'integration matches a scripted request; quality reaches the production router and pinned provider/model.',
  },
  {
    from: 'model',
    to: 'harness',
    label: 'frames + calls',
    title: 'let the harness own the turn',
    desc: 'streaming, native Functions, persistence, child sessions, and triggered work remain inside the real subject path.',
  },
  {
    from: 'harness',
    to: 'evidence',
    label: 'terminal state',
    title: 'confirm durable completion',
    desc: 'lifecycle events wake the collector; status, transcript, domain records, and complete traces establish authority.',
    event: 'harness::turn-completed',
  },
  {
    from: 'runner',
    to: 'evidence',
    label: 'collect',
    title: 'compose the track oracle',
    desc: 'integration reads exact router and trace evidence; quality adds session-tree metrics, triggered work, and domain outcomes.',
  },
  {
    from: 'evidence',
    to: 'report',
    label: 'grade',
    title: 'emit the answer the track owns',
    desc: 'integration emits a deterministic contract verdict; quality emits correctness and a separate benchmark observation.',
  },
  {
    from: 'runner',
    to: 'report',
    label: 'cleanup + classify',
    title: 'make infrastructure part of the result',
    desc: 'timeouts, setup, evidence, process, and cleanup failures stay typed and cannot disappear behind a plausible answer.',
  },
]
