/* lifecycle — the guarantee matrix as a steppable timeline (A6). */
import type { RevealStage } from '@lib/components/diagrams/StepReveal'

export const LIFECYCLE_STAGES: RevealStage[] = [
  {
    label: 'registered',
    caption: 'steady state: one live trigger per path, asset cached and served, tabs loaded.',
    rows: [
      { k: 'asset', v: 'state/page.js · 9f2b6c01…' },
      { k: 'tabs', v: 'loaded, slots populated' },
    ],
  },
  {
    label: 'worker disconnects',
    tone: 'alert',
    caption: 'the engine gcs every message-path trigger the worker registered — one UnregisterTrigger each.',
    rows: [
      { k: 'engine', v: 'gc → UnregisterTrigger' },
      { k: 'console', v: 'remove asset · push delete' },
      { k: 'tabs', v: 'dispose — ui dies with its worker' },
    ],
    note: 'no orphaned pages pointing at a dead content function. presence-gating comes free.',
  },
  {
    label: 'worker reconnects',
    caption: 'the sdk replays every registration still in its local map, original ids.',
    rows: [
      { k: 'replay', v: 'original trigger ids' },
      { k: 'console', v: 're-fetch · hash decides' },
      { k: 'tabs', v: 'hot reload only if content changed' },
    ],
  },
  {
    label: 'console restarts',
    caption: 'type re-registration replays every live binding and drains parked intents.',
    rows: [
      { k: 'registry', v: 'rebuilt from replay' },
      { k: 'hmr', v: 'hash dedupe suppresses spurious reloads' },
    ],
    note: 'register while the console is down and the engine parks the intent until it arrives.',
  },
  {
    label: 'engine restarts',
    tone: 'warn',
    caption: 'in-memory registry wiped engine-wide; console and workers reconnect and re-register.',
    rows: [
      { k: 'workers', v: 'sdk re-registers on reconnect' },
      { k: 'ordering', v: 'irrelevant — parking absorbs it' },
    ],
  },
  {
    label: 'a broken script ships',
    tone: 'alert',
    caption: 'import() rejects, no default export, or setup() throws.',
    rows: [
      { k: 'tab', v: 'console.error + one non-fatal toast' },
      { k: 'slots', v: 'contributions drop out until the next good version' },
      { k: 'console shell', v: 'unaffected — never a white screen' },
    ],
    note: 'render-time crashes are separately fenced per slot entry with the existing ErrorBoundary.',
  },
]
