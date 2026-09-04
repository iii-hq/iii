import { StepReveal } from '@lib/components/diagrams/StepReveal'
import { PageShell } from '@lib/components/PageShell'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { C, CodeBlock, K, M, S } from '@lib/components/schematic/CodeBlock'
import { FnChip } from '@lib/components/schematic/FnChip'
import { INTEGRATION_FLOOR_ROWS } from '../content/evidence'
import { READINESS_STAGES } from '../content/readiness'
import { INTEGRATION_REPORTS } from '../content/verdicts'

export function ProtocolPage() {
  return (
    <PageShell
      eyebrow="deep dive / integration"
      title="from typed fixture to exact contract verdict"
      description="the deterministic track owns one narrow Rust authoring layer, event-driven readiness, strict router generations, complete session traces, and bounded teardown."
      related={[
        { slug: 'quality', label: 'quality / e2e protocol' },
        { slug: 'scenarios', label: 'both scenario corpora' },
      ]}
    >
      <StepReveal title="readiness before stimulus" stages={READINESS_STAGES} />

      <CodeBlock title="CompiledScenarioV1">
        <K>type</K> CompiledScenarioV1 <M>{'= {'}</M>
        <br />
        {'  '}schema_version: <S>"1"</S>
        <br />
        {'  '}id: <K>string</K> <C>{'// 1 to 128 safe ASCII characters'}</C>
        <br />
        {'  '}description: <K>string</K>
        <br />
        {'  '}send: <M>{'{ session_id, message, model, provider, idempotency_key, options }'}</M>
        <br />
        {'  '}target?: <M>{'{ function_id, description, request_schema, response }'}</M>
        <br />
        {'  '}deadlines: <M>{'{ readiness_ms: '}</M>
        <S>60000</S>
        <M>{', scenario_ms: '}</M>
        <S>60000</S>
        <M>{', teardown_ms: '}</M>
        <S>15000</S>
        <M>{' }'}</M>
        <br />
        <M>{'}'}</M>
      </CodeBlock>

      <div className="grid grid-cols-1 gap-4 @4xl:grid-cols-2">
        <SpecSheet title="expansion tokens" meta="before boot">
          <SpecRow name="{{run_id}}" type="identity">
            scopes idempotency keys, target function ids, and artifact paths.
          </SpecRow>
          <SpecRow name="{{session_id}}" type="durability">
            binds send, lifecycle filtering, transcript, status, and trace queries.
          </SpecRow>
          <SpecRow name="{{system_prompt_sha256}}" type="contract">
            pins the exact built-in prompt written to expected-system-prompt.txt.
          </SpecRow>
        </SpecSheet>

        <SpecSheet title="script structure" meta="no authored YAML">
          <SpecRow name="CompiledFixtureV1" type="in-memory">
            compiled scenario, router script, and system prompt template.
          </SpecRow>
          <SpecRow name="ScenarioFixture" type="runner-only">
            slug, driver, expected terminal turns, and verification function.
          </SpecRow>
          <SpecRow name="ScriptedGenerationV1" type="ordered">
            ordinal, twelve explicit matchers, frames, and RouterChatResponse.
          </SpecRow>
        </SpecSheet>
      </div>

      <SpecSheet title="the common floor" meta="runs before scenario verification">
        {INTEGRATION_FLOOR_ROWS.map((row) => (
          <SpecRow key={row.name} name={row.name} type={row.type}>
            {row.desc}
          </SpecRow>
        ))}
      </SpecSheet>

      <div className="grid grid-cols-1 gap-4 @4xl:grid-cols-2">
        <SpecSheet title="trace span identities" meta="normative">
          <SpecRow name="execute <run_id>::<alias>" type="controlled call">
            invocation input is normalized by removing engine-injected top-level underscore fields.
          </SpecRow>
          <SpecRow name="execute integration-probe::turn-completed" type="lifecycle">
            the payload must be complete, untruncated, terminal, and bound to a collected turn.
          </SpecRow>
          <div className="mt-3 flex flex-wrap gap-2">
            <FnChip>iii.invocation.input</FnChip>
            <FnChip>iii.invocation.output</FnChip>
            <FnChip>iii.payload.json</FnChip>
          </div>
        </SpecSheet>

        <SpecSheet title="report contracts" meta="canonical JSON">
          {INTEGRATION_REPORTS.map((row) => (
            <SpecRow key={row.name} name={row.name} type={row.type}>
              {row.desc}
            </SpecRow>
          ))}
        </SpecSheet>
      </div>
    </PageShell>
  )
}
