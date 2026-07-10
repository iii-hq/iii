/**
 * spawn.ts — the child/state data SpawnSection feeds the shared SpawnTree
 * archetype: a parent turn fanning out to three sub-agents and joining back.
 */

import type { SpawnParentState, SpawnTreeChild, SpawnTreeState } from '@lib/components/diagrams/SpawnTree'

export const SPAWN_CHILDREN: SpawnTreeChild[] = [
  {
    id: 'a',
    label: 'sub-agent a — s_9c1',
    task: 'task: survey the provider landscape',
    meta: 'policy: parent ∩ request · output: json + schema',
    x: 80,
  },
  {
    id: 'b',
    label: 'sub-agent b — s_9c2',
    task: 'task: compare pricing + limits',
    meta: 'policy: parent ∩ request · output: json + schema',
    x: 380,
  },
  {
    id: 'c',
    label: 'sub-agent c — s_9c3',
    task: 'task: draft the recommendation',
    meta: 'policy: parent ∩ request · output: json + schema',
    x: 680,
  },
]

export const SPAWN_PARENT_LABELS: Record<SpawnParentState, string> = {
  running: 'running — generating',
  parked: 'parked — awaiting_functions · zero queue steps held',
  resumed: 'running — reasoning over results',
  completed: 'completed — result delivered',
}

export const SPAWN_STATES: SpawnTreeState[] = [
  {
    title: 'the model asks for three experts',
    desc: 'harness::spawn is just another function call on the one invocation surface — no special protocol. each call names a task, an optional model, a policy request, and a typed output contract: "give me json matching this schema."',
    parent: 'running',
    children: ['hidden', 'hidden', 'hidden'],
    joins: [false, false, false],
  },
  {
    title: 'children spawn — the parent parks',
    desc: 'each child is an ordinary session with its own turn, linked to the parent by metadata. the spawn dispatch reports pending and the parent parks — durable execution means it holds no queue step, costs nothing, and cannot be lost while it waits.',
    parent: 'parked',
    children: ['queued', 'queued', 'queued'],
    joins: [false, false, false],
  },
  {
    title: 'three agents, truly parallel',
    desc: 'the turn queue orders steps per session and runs sessions concurrently — spawn three children, get three loops working at once. every child transcript is live: bind the session events filtered by parent_session_id and watch them think.',
    parent: 'parked',
    children: ['running', 'running', 'running'],
    joins: [false, false, false],
  },
  {
    title: 'the first result joins',
    desc: "a child that completes resolves the parent's call with its typed deliverable — schema-validated json when the contract asks for it. the result lands under a deterministic entry id, so a replayed completion can never double-deliver.",
    parent: 'parked',
    children: ['running', 'done', 'running'],
    joins: [false, true, false],
  },
  {
    title: 'all results in — the parent wakes',
    desc: 'the last resolve re-enqueues the parent turn exactly where it parked. the model now sees three function_results in its transcript and reasons over all of them at once.',
    parent: 'resumed',
    children: ['done', 'done', 'done'],
    joins: [true, true, true],
  },
  {
    title: 'one answer, a tree of work',
    desc: "depth, fan-out, and turn budgets bound the whole tree; a child's policy is the parent's intersected with its request — narrow, never escalate. stop the parent and the cancellation cascades to every descendant.",
    parent: 'completed',
    children: ['done', 'done', 'done'],
    joins: [true, true, true],
  },
]
