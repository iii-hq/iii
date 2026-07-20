/* hot edit — the end-to-end reload sequence from hot-reload.md (A5). */
import type { SeqLane, SeqStep } from '@lib/components/diagrams/SequencePlayer'

export const SEQ_LANES: SeqLane[] = [
  { id: 'dev', label: 'dev', x: 80 },
  { id: 'worker', label: 'worker', x: 270 },
  { id: 'engine', label: 'engine', x: 470 },
  { id: 'console', label: 'console worker', x: 670 },
  { id: 'tab', label: 'each open tab', x: 880 },
]

export const SEQ_STEPS: SeqStep[] = [
  {
    from: 'dev',
    to: 'worker',
    label: 'save ui/page.tsx',
    title: 'rebuild',
    desc: 'console-build --watch rebuilds dist/ui/page.js; the watch hook re-registers the same path, then unregisters the previous handle.',
  },
  {
    from: 'worker',
    to: 'engine',
    label: 'RegisterTrigger {path: "state/page.js"}',
    title: 'registration is deployment',
    desc: 'a fresh uuid trigger id for the same config.path — the engine never dedupes triggers by content; identity is the id alone.',
    event: 'console:script',
  },
  {
    from: 'engine',
    to: 'console',
    label: 'forward to type owner',
    title: 'ownership is the channel',
    desc: 'the console registered the console:script type, so every registration of it is pushed to the console’s websocket immediately. no new engine surface.',
  },
  {
    from: 'console',
    to: 'worker',
    label: 'state::ui-content {path} → {content}',
    title: 'fetch the bytes',
    desc: 'a script trigger never fires. its function_id names the content function the console invokes over the bus: 2 attempts × 3s, inside the 10s ack window.',
  },
  {
    from: 'console',
    to: 'console',
    label: 'sha256 → supersede old id',
    title: 'path-keyed override',
    desc: 'same path ⇒ override. the superseded engine row is pruned (engine::unregister_trigger), keeping at most one live trigger per path.',
  },
  {
    from: 'console',
    to: 'tab',
    label: 'push {event: set, path, kind, hash}',
    title: 'per subscribed tab',
    desc: 'each tab registered a console:assets trigger at boot and got a sync push back. the console now invokes every subscriber’s iii::console::ui-assets handler — plain bus calls, span-suppressed by the iii:: prefix. no stream worker involved.',
    event: 'console:assets',
  },
  {
    from: 'tab',
    to: 'tab',
    label: 'dispose → import(?v=hash) → setup(host)',
    title: 'hot swap',
    desc: 'old cleanups run lifo, the new module re-registers its slots. the console shell never reloads — this design never issues a full page reload.',
  },
]
