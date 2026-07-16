/* why — today's failures (A2). data only. */

export const PAIN_CARDS = [
  {
    n: '01',
    title: 'real models flake contract tests',
    body: 'put a live model behind a regression suite and every provider hiccup, retry, and sampling wobble reads as a harness bug. the durability contracts never get a clean signal.',
  },
  {
    n: '02',
    title: 'scripted stacks cannot judge quality',
    body: 'a deterministic router proves ordering, streaming, and durability. it cannot say whether a representative workflow actually succeeded for a user.',
  },
  {
    n: '03',
    title: 'a skipped dependency reads as green',
    body: 'when a missing worker, key, or fixture silently skips a test, the dashboard says pass while nothing ran. the worst failure mode a gate can have.',
  },
  {
    n: '04',
    title: 'the agent grades itself',
    body: 'a subject’s own “done” is not evidence. outcome correctness has to be independent of the agent’s claims, or the evaluation measures confidence, not success.',
  },
] as const

export const SPLIT_TERMS = [
  {
    name: 'shared',
    type: 'vocabulary · report conventions',
    desc: 'both tracks name scenarios, evidence, and classifications the same way, so a report reads the same either side.',
  },
  {
    name: 'never shared',
    type: 'oracle · execution owner · release policy',
    desc: 'a deterministic runner and a real-model evaluator answer different questions; blending them is how a gate starts lying.',
  },
  {
    name: 'both enter through',
    type: 'harness::send',
    desc: 'ordinary public turns. neither track seeds private harness state or calls harness::turn as a continuation api.',
  },
] as const
