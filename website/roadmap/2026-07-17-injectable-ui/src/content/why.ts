/* why — the four compile-time extension paths as they exist today (A2). */

export const WHY_CARDS = [
  {
    title: '13 hand-chained renderers',
    body: 'per-function chat renderers are 13 ToolView families chained with ?? in FunctionCallCard.tsx (:279-307). adding one means editing console source, rebuilding, releasing the console.',
  },
  {
    title: '26 files for one page',
    body: 'worker pages compile into web/ and gate on worker presence via buildViewOptions. the memory page touched 26 console files, 23 of them under web/src (workers commit 2e31cb4b).',
  },
  {
    title: 'hardcoded composer pickers',
    body: 'every composer picker is a boolean prop wired from ChatView by worker presence (Composer.tsx:332-375). a new picker for a new worker is a console pr, not a worker deploy.',
  },
  {
    title: 'one form for every worker',
    body: 'configuration renders through a single structural json-schema form (SchemaForm.tsx:21-31). a worker with a value but no schema gets no form at all — per-worker forms are impossible.',
  },
] as const

export const WHY_NOTE = {
  headline: 'the registry the console’s own docs sketch is marked "not implemented"',
  detail:
    'custom-function-call-message.md §12 sketches FunctionCallRenderer and stops. this spec is that registry, generalized to four slot kinds and made runtime-loadable — nothing like it exists in either repo today.',
} as const
