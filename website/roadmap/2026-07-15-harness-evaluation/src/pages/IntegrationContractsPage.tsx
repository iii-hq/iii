import { PageShell } from '@lib/components/PageShell'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { C, CodeBlock, K, M, S } from '@lib/components/schematic/CodeBlock'
import { FnChip } from '@lib/components/schematic/FnChip'
import { AUTHORING_LAYERS, RECORDER_PLANE, SUPERVISOR_STEPS } from '../content/protocols'
import { EXPANSION, QUARANTINE } from '../content/contract'

/**
 * A14 — deep dive on the integration test-support contracts: the authored →
 * compiled scenario layers, the compiled script schema, the recorder control
 * plane, and the supervisor. Test-support api owned by the runner; production
 * ids stay fixed.
 */
export function IntegrationContractsPage() {
  return (
    <PageShell
      eyebrow="deep dive · integration tests"
      title="the test-support contracts"
      description="the scenario compiler, scripted router, recorder, and supervisor are test support outside the subject path, owned by the integration runner. they mirror existing wire contracts (cited file:line in the spec), versioned and strict: schemas deny unknown fields, and fixture problems are runner errors before the stack even starts."
      related={[{ slug: 'e2e-protocol', label: 'e2e protocol' }]}
    >
      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          authoring and compilation · one source module per scenario
        </div>
        <CodeBlock title="src/scenarios/streamed_text.rs · builders producing data, resolved before boot">
          <K>AuthoredScenario</K>::new(<S>&quot;streamed-text&quot;</S>,{' '}
          <S>&quot;streamed text reaches durable completion&quot;</S>)
          {'\n  '}.send(<K>Send</K>::message(<S>&quot;Return the fixture phrase.&quot;</S>).allow(<M>[]</M>))
          {'   '}
          <C>{'// [] disables dispatch'}</C>
          {'\n  '}.generation(<K>Reply</K>::text(<S>&quot;fixture complete&quot;</S>).chunks(<M>[</M>
          <S>&quot;fixture &quot;</S>, <S>&quot;complete&quot;</S>
          <M>]</M>).usage(<K>8</K>, <K>2</K>))
          {'\n\n'}
          <C>{'// no yaml layer: a fixture mistake fails cargo build first.'}</C>
          {'\n'}
          <C>{'// the compiler expands this into CompiledFixtureV1: all twelve'}</C>
          {'\n'}
          <C>{'// matchers explicit, literal wire frames, derived recorder config.'}</C>
        </CodeBlock>
        <div className="mt-4 border border-rule bg-bg flex flex-col">
          {AUTHORING_LAYERS.map((row) => (
            <SpecRow key={row.name} name={row.name} type={row.type}>
              {row.desc}
            </SpecRow>
          ))}
        </div>
      </div>

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          the compiled script, sketched
        </div>
        <CodeBlock title="RouterScriptV1 · one strict script per scenario">
          <M>interface</M> <K>RouterScriptV1</K> <M>{'{'}</M>
          {'\n  '}schema_version: <S>&quot;1&quot;</S>
          {'\n  '}scenario_id: <K>string</K>
          {'\n  '}model: <K>ModelFixtureV1</K>
          {'      '}
          <C>{'// pinned fixture; models::* are projections of it'}</C>
          {'\n  '}generations: <K>ScriptedGenerationV1</K>[]
          {'\n'}
          <M>{'}'}</M>
          {'\n\n'}
          <M>interface</M> <K>ScriptedGenerationV1</K> <M>{'{'}</M>
          {'\n  '}ordinal: <K>number</K>
          {'            '}
          <C>{'// consumed strictly in order'}</C>
          {'\n  '}match: <M>{'{ '}</M>writer_ref, request_id, model, provider, system_prompt,
          {'\n          '}messages, tools, response_format, thinking_level,
          {'\n          '}max_output_tokens, provider_options, metadata <M>{'}'}</M>
          {'  '}
          <C>{'// all twelve, each an explicit JsonMatcherV1'}</C>
          {'\n  '}frames: <K>AssistantMessageEvent</K>[]
          {'   '}
          <C>{'// exactly one terminal frame'}</C>
          {'\n  '}response: <K>RouterChatResponse</K>
          {'\n'}
          <M>{'}'}</M>
        </CodeBlock>
        <p className="mt-3 font-mono text-[12px] leading-[1.7] text-ink-faint lowercase max-w-[72ch]">
          an unexpected subject call, a matcher failure, or an unused expectation is a{' '}
          <span className="text-ink">contract_failure</span>. duplicate ordinals, an invalid matcher, extra
          generations, or response/frame disagreement reject the script as <span className="text-ink">runner_error</span>{' '}
          before boot. normalizers apply to copies, never to stored evidence; fixtures never depend on wall-clock
          time. barriers are gone from v1: a fault that must land after a target call uses a compiler-derived
          deterministic gate (<span className="text-ink">fault.after_target_calls</span>): the recorder fsyncs the
          event, signals the gate, the runner applies the fault, then releases the held response on every success
          or error path.
        </p>
      </div>

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          the recorder · a private control plane, two engine-visible handlers
        </div>
        <div className="border border-rule bg-bg flex flex-col">
          {RECORDER_PLANE.map((row) => (
            <div key={row.name} className="px-4 py-3 border-b border-rule-2 last:border-b-0">
              <div className="flex flex-wrap items-baseline gap-x-3">
                <FnChip>{row.name}</FnChip>
                <span className="font-mono text-[10.5px] uppercase tracking-[0.06em] text-ink-ghost">{row.type}</span>
              </div>
              <p className="mt-1.5 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{row.desc}</p>
            </div>
          ))}
          <div className="px-4 py-3 border-t border-rule font-mono text-[11.5px] leading-[1.6] text-ink-faint lowercase">
            a run-scoped target may only use an id prefixed by its own run; production ids stay fixed. keeping the
            control plane out of the engine means durable evidence stays readable even after an engine crash.
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <div>
          <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
            the supervisor cli
          </div>
          <CodeBlock title="explicit binaries · the runner never downloads">
            harness-integration <K>run</K> <M>\</M>
            {'\n  '}--engine-bin <S>&lt;path&gt;</S> <M>\</M>
            {'   '}
            <C>{'// or III_BIN'}</C>
            {'\n  '}--harness-bin <S>&lt;path&gt;</S> <M>\</M>
            {'\n  '}--worker-bin <S>&lt;name=path&gt;</S>... <M>\</M>
            {'\n  '}--scenario <S>&lt;id|all&gt;</S> <M>\</M>
            {'\n  '}[--repeat <S>&lt;n&gt;</S>] <M>\</M>
            {'\n  '}--artifacts-dir <S>&lt;path&gt;</S>
            {'\n\n'}
            <C>{'// every executable: absolute path + sha-256, plus the'}</C>
            {'\n'}
            <C>{'// exact engine.lock contents, recorded before boot.'}</C>
            {'\n'}
            <C>{'// an unreadable binary is a setup failure; an error'}</C>
            {'\n'}
            <C>{'// string is never written into a digest field.'}</C>
          </CodeBlock>
        </div>
        <div>
          <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
            per scenario, in order
          </div>
          <div className="border border-rule bg-bg flex flex-col">
            {SUPERVISOR_STEPS.map((step, i) => (
              <div key={step} className="flex items-baseline gap-x-3 px-4 py-2.5 border-b border-rule-2 last:border-b-0">
                <span className="font-mono text-[11px] text-ink-ghost tabular-nums shrink-0">{i + 1}</span>
                <span className="font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{step}</span>
              </div>
            ))}
          </div>
        </div>
      </div>

      <SpecSheet title="offline cassette tooling" meta="outside the v1 runner and gate">
        <p className="font-mono text-[12px] leading-[1.7] text-ink-faint lowercase">
          a capture schema and sanitizer without a real capture → sanitize → verify → import workflow would be
          unused surface and could give false confidence about secret handling. when that workflow exists it lives
          in isolated offline tooling: versioned provenance (engine, harness, router, provider, model), canonical
          content hashing, and a denylist that rejects credentials, cookies, personal data, unstable ids, and
          provider-private metadata. no captured artifact becomes a committed fixture until schema round-trip,
          sanitization, and denylist tests pass.
        </p>
      </SpecSheet>

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          the expansion order after version 1
        </div>
        <div className="border border-rule bg-bg flex flex-col">
          {EXPANSION.map((row) => (
            <div key={row.order} className="grid grid-cols-[64px_minmax(0,1fr)] gap-x-4 px-4 py-3 border-b border-rule-2 last:border-b-0">
              <span className="font-mono text-[12px] text-ink">{row.order}</span>
              <span className="font-mono text-[12px] leading-[1.7] text-ink-faint lowercase">{row.items}</span>
            </div>
          ))}
          <div className="grid grid-cols-[64px_minmax(0,1fr)] gap-x-4 px-4 py-3 border-b border-rule-2">
            <span className="font-mono text-[12px] text-warn">held</span>
            <span className="font-mono text-[12px] leading-[1.7] text-ink-faint lowercase">
              {QUARANTINE.ids.toUpperCase()} — {QUARANTINE.desc}
            </span>
          </div>
          <div className="px-4 py-3 border-t border-rule font-mono text-[11.5px] leading-[1.6] text-ink-faint lowercase">
            engine.lock pins the exact engine revision; ci checks it out, builds it with cargo --locked, and
            records the binary&apos;s sha-256 — no download or skip fallback. the non-required job runs on every
            pull request, executes each scenario twice, and requires byte-identical result.json. promotion to a
            required check needs 100 consecutive clean runs across 14 days, zero skips, zero unexplained flakes;
            stack reuse is not a condition, and never happens in v1.
          </div>
        </div>
      </div>
    </PageShell>
  )
}
