/* wire — the override algorithm as a steppable lifecycle (A6). */
import type { RevealStage } from '@lib/components/diagrams/StepReveal'

export const WIRE_STAGES: RevealStage[] = [
  {
    label: 'validate',
    caption: 'path rules enforced by the console handler; violations reject the registration.',
    rows: [
      { k: 'path', v: 'state/page.js' },
      { k: 'pattern', v: '^[a-z0-9][a-z0-9._-]*(/…)*$' },
      { k: 'extension', v: '.js matches console:script' },
    ],
    note: 'no dot segments, no leading slash, no backslashes — it becomes a url segment under /ui/.',
  },
  {
    label: 'fetch',
    caption: 'invoke the trigger’s function_id — the content function — over the bus.',
    rows: [
      { k: 'function', v: 'state::ui-content' },
      { k: 'budget', v: '2 × 3s + 250ms backoff' },
      { k: 'window', v: 'inside the engine’s 10s ack' },
    ],
    note: 'live-registration failure rejects the registration; the error reaches the registrant.',
  },
  {
    label: 'hash',
    caption: 'sha256(content), first 16 hex chars — the asset’s version everywhere it travels.',
    rows: [
      { k: 'hash', v: '9f2b6c01d4e8aa17' },
      { k: 'dedupe', v: 'same id + hash → no-op' },
    ],
    note: 'replay re-delivers accepted bindings; the hash absorbs them silently.',
  },
  {
    label: 'supersede',
    tone: 'warn',
    caption: 'same path, different trigger id: last writer wins, the old engine row is pruned.',
    rows: [
      { k: 'by_path', v: 'state/page.js → new id' },
      { k: 'old id', v: 'engine::unregister_trigger' },
      { k: 'steady state', v: '≤ 1 live trigger per path' },
    ],
    note: 'pruning makes console-restart replay order irrelevant — no tiebreaker needed.',
  },
  {
    label: 'push',
    tone: 'accent',
    caption: 'commit both maps, then push to every console:assets subscriber; tabs take it from here.',
    rows: [
      { k: 'push', v: 'set {path, kind, hash}' },
      { k: 'to', v: 'every subscribed tab, fire-and-forget' },
      { k: 'ack', v: 'sent after the handler returns' },
    ],
  },
]
