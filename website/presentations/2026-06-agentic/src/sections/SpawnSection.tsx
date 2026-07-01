import { SpawnTree } from '@lib/components/diagrams/SpawnTree'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { C, CodeBlock, K, M, S } from '@lib/components/schematic/CodeBlock'
import { SPAWN_CHILDREN, SPAWN_PARENT_LABELS, SPAWN_STATES } from '../content/spawn'

export function SpawnSection() {
  return (
    <Section
      id="spawn"
      index="07"
      eyebrow="sub-agents"
      title="spawn is one function call — children are ordinary sessions, parallel by construction."
      lede="no orchestrator process, no second framework: a parent turn fans out bounded children and joins on their typed results, all on the same durable queue and the same reactive surface as everything else."
    >
      <SpawnTree
        heading="harness::spawn — fan out, join back"
        chips={['depth ≤ 3', 'fan-out ≤ 5', 'turns ≤ parent budget']}
        parentTitle="parent turn — s_7a1 / t_004"
        parentLabels={SPAWN_PARENT_LABELS}
        parentCallLine={{ first: 'model emits: harness::spawn ×3', rest: 'calls: 3 × harness::spawn' }}
        nodes={SPAWN_CHILDREN}
        states={SPAWN_STATES}
        ariaLabel="a parent agent spawning three parallel sub-agents and joining their results"
      />

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4 items-start">
        <CodeBlock title="what the model actually emits">
          <K>agent_trigger</K> <M>{'{'}</M>
          {'\n'}
          {'  '}function: <S>"harness::spawn"</S>,{'\n'}
          {'  '}payload: <M>{'{'}</M>
          {'\n'}
          {'    '}task: <S>"compare pricing + limits across providers"</S>,{'\n'}
          {'    '}options: <M>{'{'}</M>
          {'\n'}
          {'      '}output: <M>{'{'}</M> type: <S>"json"</S>, schema: <M>{'{ … }'}</M> <M>{'}'}</M>,{'\n'}
          {'      '}functions: <M>{'{'}</M> allow: [<S>"search::web"</S>, <S>"router::models::list"</S>] <M>{'}'}</M>
          {'\n'}
          {'    '}
          <M>{'}'}</M>
          {'\n'}
          {'  '}
          <M>{'}'}</M>
          {'\n'}
          <M>{'}'}</M>
          {'\n\n'}
          <C>// the child's allow-list is intersected with the parent's —</C>
          {'\n'}
          <C>// narrow, never escalate. linkage is injected by the harness,</C>
          {'\n'}
          <C>// never trusted from model arguments.</C>
        </CodeBlock>

        <SpecSheet title="the spawn dispatch, step by step" meta="5 moves" defaultOpen>
          <div className="flex flex-col">
            <SpecRow name="1 — guards" type="fail as results, not throws">
              spawn must itself be allowed; depth and fan-out caps refuse politely — the model reads the error and
              adapts.
            </SpecRow>
            <SpecRow name="2 — policy subsetting" type="parent ∩ request">
              children can narrow their reach, never widen it; turn budgets cap at the parent's remainder.
            </SpecRow>
            <SpecRow name="3 — child session created" type="linkage in metadata">
              parent_session_id / parent_turn_id / call id / depth — any ui rebuilds the tree, and event filters can
              target one branch.
            </SpecRow>
            <SpecRow name="4 — parent parks" type="pending call">
              the same durable-execution primitive as approvals — no queue step held while children work.
            </SpecRow>
            <SpecRow name="5 — completion resolves" type="typed join">
              completed delivers the contract result; failed or cancelled deliver an explicit error. deterministic ids
              make double-resolution impossible.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
