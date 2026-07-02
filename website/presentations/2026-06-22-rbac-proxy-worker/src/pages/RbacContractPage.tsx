import { PageShell } from '@lib/components/PageShell'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { C, CodeBlock, M, S } from '@lib/components/schematic/CodeBlock'
import {
  AUTH_INPUT_FIELDS,
  AUTH_RESULT_FIELDS,
  DIVERGENCES,
  HOOK_ROWS,
  MIDDLEWARE_FIELDS,
  RESOLUTION_RULES,
} from '../content/rbac'

/**
 * A14 — deep dive on the rbac contract: the behavioural promise is parity with
 * a worker-gateway listener, by vendoring the engine's decision code. This page
 * states the auth/expose/middleware/hook surface and the few honest divergences.
 */
export function RbacContractPage() {
  return (
    <PageShell
      eyebrow="deep dive"
      title="the rbac contract"
      description="a connection through rbac-proxy is gated exactly as the same connection through an engine rbac listener would be. the decision logic is vendored verbatim from the engine, so there is no second, drifting copy of the rules, only a second home for them."
      related={[{ slug: 'engine-overrides', label: 'discovery overrides' }]}
    >
      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          access resolution order
        </div>
        <div className="border border-rule bg-bg flex flex-col">
          {RESOLUTION_RULES.map((rule) => (
            <div key={rule.n} className="flex items-baseline gap-x-3 px-4 py-3 border-b border-rule-2 last:border-b-0">
              <span className="font-mono text-[11px] text-ink-ghost tabular-nums shrink-0">{rule.n}</span>
              <span className="font-mono text-[13px] text-ink lowercase flex-1 min-w-0">{rule.text}</span>
              <span
                className={`font-mono text-[11px] uppercase tracking-[0.08em] shrink-0 ${
                  rule.verdict === 'allow' ? 'text-accent' : 'text-alert'
                }`}
              >
                {rule.verdict}
              </span>
            </div>
          ))}
        </div>
      </div>

      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="AuthInput" meta="built from the upgrade" defaultOpen>
          <div className="flex flex-col">
            {AUTH_INPUT_FIELDS.map((f) => (
              <SpecRow key={f.name} name={f.name} type={f.type}>
                {f.desc}
              </SpecRow>
            ))}
          </div>
        </SpecSheet>

        <SpecSheet title="AuthResult" meta="the session boundaries" defaultOpen>
          <div className="flex flex-col">
            {AUTH_RESULT_FIELDS.map((f) => (
              <SpecRow key={f.name} name={f.name} type={f.type}>
                {f.desc}
              </SpecRow>
            ))}
          </div>
        </SpecSheet>
      </div>

      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <CodeBlock title="expose_functions · two filter shapes, mixable">
          <M>expose_functions:</M>
          {'\n  - '}
          <S>match(&quot;api::*&quot;)</S>
          {'        '}
          <C>{'# wildcard, anchored'}</C>
          {'\n  - '}
          <S>match(&quot;*::public&quot;)</S>
          {'\n  - '}
          <M>metadata:</M>
          {'             '}
          <C>{'# all keys AND; filters OR'}</C>
          {'\n      '}public: <S>true</S>
        </CodeBlock>

        <SpecSheet title="middleware" meta="its return is the result" defaultOpen>
          <div className="flex flex-col">
            {MIDDLEWARE_FIELDS.map((f) => (
              <SpecRow key={f.name} name={f.name} type={f.type}>
                {f.desc}
              </SpecRow>
            ))}
          </div>
          <p className="mt-3 font-mono text-[12px] leading-[1.7] text-ink-faint lowercase">
            whatever the middleware returns is returned to the caller. if it wants the target to actually run, it must
            invoke it itself. engine:: calls bypass middleware entirely.
          </p>
        </SpecSheet>
      </div>

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">registration hooks</div>
        <div className="border border-rule bg-bg overflow-x-auto">
          <div className="min-w-[560px] flex flex-col">
            <div className="grid grid-cols-[280px_180px_1fr] gap-x-4 bg-panel px-4 py-2.5 border-b border-rule font-mono text-[10px] uppercase tracking-[0.12em] text-ink-faint">
              <span>hook</span>
              <span>fires on</span>
              <span>result (omitted = unchanged)</span>
            </div>
            {HOOK_ROWS.map((row) => (
              <div
                key={row.hook}
                className="grid grid-cols-[280px_180px_1fr] gap-x-4 px-4 py-2.5 border-b border-rule-2 last:border-b-0 font-mono text-[12px] leading-[1.5]"
              >
                <span className="text-ink">{row.hook}</span>
                <span className="text-ink-faint">{row.firesOn}</span>
                <span className="text-ink-ghost">{row.result}</span>
              </div>
            ))}
          </div>
        </div>
        <p className="mt-3 font-mono text-[12px] leading-[1.7] text-ink-faint lowercase">
          each hook sees the bare id plus the session context, before prefix resolution. it returns mapped fields to
          allow, or throws to deny.
        </p>
      </div>

      <CodeBlock title="full example · configuration: rbac-proxy">
        host: <S>0.0.0.0</S>
        {'\n'}
        port: <S>49200</S>
        {'\n'}
        engine_url: <S>ws://127.0.0.1:49134</S>
        {'\n'}
        expose_worker_internals: <S>false</S>
        {'\n'}
        middleware_function_id: <S>my-project::middleware-function</S>
        {'\n'}
        <M>rbac:</M>
        {'\n  '}auth_function_id: <S>my-project::auth-function</S>
        {'\n  '}on_function_registration_function_id: <S>my-project::on-function-reg</S>
        {'\n  '}on_trigger_registration_function_id: <S>my-project::on-trigger-reg</S>
        {'\n  '}expose_functions:
        {'\n    - '}
        <S>match(&quot;api::*&quot;)</S>
        {'\n    - '}
        <S>match(&quot;*::public&quot;)</S>
      </CodeBlock>

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          intentional divergences from the engine
        </div>
        <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-px bg-rule border border-rule">
          {DIVERGENCES.map((d) => (
            <div key={d.title} className="bg-bg p-5 min-w-0">
              <div className="font-mono text-[14px] font-semibold text-ink lowercase mb-2.5">{d.title}</div>
              <div className="flex flex-col gap-y-1.5 font-mono text-[12px] leading-[1.6]">
                <div className="text-ink-faint lowercase">
                  <span className="text-ink-ghost">engine: </span>
                  {d.engine}
                </div>
                <div className="text-accent lowercase">
                  <span className="text-ink-ghost">proxy: </span>
                  {d.proxy}
                </div>
                <div className="text-ink-faint lowercase pt-1">
                  <span className="text-ink-ghost">why: </span>
                  {d.why}
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </PageShell>
  )
}
