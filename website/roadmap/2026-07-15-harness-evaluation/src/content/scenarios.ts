import { QUALITY_SCENARIOS } from './quality'

export type ScenarioTrack = 'integration' | 'quality'

export interface EvaluationScenario {
  id: string
  slug: string
  meta: string
  stimulus: string
  outcome: string
  proof: readonly string[]
}

export const INTEGRATION_SCENARIOS: EvaluationScenario[] = [
  {
    id: 'E2E-001',
    slug: 'streamed-text',
    meta: 'direct · 1 turn',
    stimulus: 'Return the fixture phrase.',
    outcome: 'fixture complete',
    proof: ['2 text deltas', 'usage 8 / 2', 'no duplicate entry id'],
  },
  {
    id: 'E2E-002',
    slug: 'exactly-once-function',
    meta: 'direct · 1 turn',
    stimulus: 'Call the controlled target once.',
    outcome: 'recorded once',
    proof: ['1 target span', '2 generations', 'result reaches generation 2'],
  },
  {
    id: 'UI-001',
    slug: 'console-streamed-text',
    meta: 'playground · 1 turn',
    stimulus: 'Console sends the fixture phrase.',
    outcome: 'console fixture complete',
    proof: ['production Console', 'agent_trigger contract', 'no duplicate entry id'],
  },
  {
    id: 'UI-002',
    slug: 'multi-turn-traces',
    meta: 'playground · 2 turns',
    stimulus: 'Compiled function send, then one Console message.',
    outcome: 'recorded once + second trace complete',
    proof: ['2 complete traces', 'request steps 0 · 1 · 0', '2 final answers'],
  },
]

export const QUALITY_EVALUATION_SCENARIOS: EvaluationScenario[] = QUALITY_SCENARIOS.map((scenario) => ({
  id: scenario.id,
  slug: scenario.slug,
  meta: 'real model · one pinned subject',
  stimulus: 'Scenario-local prompt, functions, fixtures, validators, and budgets.',
  outcome: scenario.outcome,
  proof: scenario.proof,
}))

export const SCENARIO_TRACKS: Record<ScenarioTrack, EvaluationScenario[]> = {
  integration: INTEGRATION_SCENARIOS,
  quality: QUALITY_EVALUATION_SCENARIOS,
}
