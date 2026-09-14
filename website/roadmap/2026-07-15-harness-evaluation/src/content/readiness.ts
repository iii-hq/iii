import type { RevealStage } from '@lib/components/diagrams/StepReveal'

export const READINESS_STAGES: RevealStage[] = [
  {
    label: 'register sinks',
    caption: 'the in-process probe exposes four strict observer functions before the harness exists.',
    rows: [
      { k: 'ready', v: 'integration-probe::harness-ready' },
      { k: 'registry', v: 'integration-probe::functions-changed' },
      { k: 'trace', v: 'integration-probe::traces-changed' },
      { k: 'terminal', v: 'integration-probe::turn-completed' },
    ],
  },
  {
    label: 'bind early',
    caption:
      'engine triggers are registered through acknowledged RPCs; harness-owned types may remain deferred until boot.',
    rows: [
      { k: 'functions', v: 'active' },
      { k: 'trace', v: 'active' },
      { k: 'completed', v: 'deferred' },
      { k: 'ready', v: 'deferred' },
    ],
    note: 'the probe connection is also the registration barrier.',
  },
  {
    label: 'start harness',
    caption: 'the real worker provisions its queue and installs lifecycle and configuration bindings.',
    rows: [
      { k: 'harness', v: 'booting' },
      { k: 'stimulus', v: 'blocked' },
    ],
  },
  {
    label: 'ready event',
    tone: 'accent',
    caption:
      'harness::ready emits only after boot initialization is complete; a late binding receives the retained ready state.',
    rows: [
      { k: 'status', v: 'ready' },
      { k: 'timestamp', v: 'now_ms' },
    ],
  },
  {
    label: 'function floor',
    caption: 'the latest functions snapshot must contain the three minimum public and dependency contracts.',
    rows: [
      { k: 'required', v: 'harness::send' },
      { k: 'required', v: 'session::messages' },
      { k: 'required', v: 'context::assemble' },
    ],
  },
  {
    label: 'confirm binding',
    tone: 'accent',
    caption: 'the runner replaces the stale deferred completion binding with one acknowledged active session binding.',
    rows: [
      { k: 'filter', v: 'session_id = current run' },
      { k: 'stimulus', v: 'unblocked' },
    ],
    note: 'no broad descriptor, config, or queue-topic polling is needed.',
  },
]

export const READY_CONTRACT = [
  { name: 'config', type: '{}', desc: 'unknown fields are rejected.' },
  { name: 'payload.status', type: '"ready"', desc: 'the probe rejects any other state.' },
  { name: 'payload.timestamp', type: 'integer', desc: 'emitted by the harness as now_ms.' },
] as const
