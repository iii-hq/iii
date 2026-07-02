import { Section } from '@lib/components/Section'
import { Button } from '@lib/components/schematic/Button'
import { C, CodeBlock, K, M, S } from '@lib/components/schematic/CodeBlock'
import { StatusPanel } from '@lib/components/schematic/StatusPanel'
import { useState } from 'react'

type DoorState = 'waiting' | 'admitted' | 'rejected'

const FAIL_RULES = [
  { fn: 'auth', when: 'unresolvable / throws', then: 'reject the upgrade: error frame + close' },
  { fn: 'middleware', when: 'unresolvable / throws', then: 'deny the call: InvocationResult error' },
  { fn: 'hook', when: 'unresolvable / throws', then: 'deny the registration' },
]

/**
 * A10 — a decision flow proving the deny-by-default posture. Drive the auth
 * function's outcome and watch the door: a resolved result admits the
 * connection; anything else closes it before the upstream is ever opened.
 */
export function FailClosedSection() {
  const [state, setState] = useState<DoorState>('waiting')

  return (
    <Section
      id="fail-closed"
      index="05"
      eyebrow="fail closed"
      title="a broken policy function denies. it never opens the door."
      lede="unset and unresolvable are not the same. no auth function means a permissive default where expose alone gates. a configured function that throws, errors, or cannot be reached always denies."
    >
      <div className="grid grid-cols-1 @4xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-4">
        <div className="border border-rule bg-bg">
          <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
            an upgrade arrives · auth_function_id is set
          </div>

          <div className="p-4 flex flex-col gap-4">
            <div className="flex flex-wrap items-center gap-2.5">
              <Button onClick={() => setState('admitted')} size="sm">
                auth resolves
              </Button>
              <Button variant="pill" size="sm" onClick={() => setState('rejected')}>
                auth throws or unreachable
              </Button>
              <Button variant="ghost" size="sm" onClick={() => setState('waiting')} disabled={state === 'waiting'}>
                replay
              </Button>
            </div>

            {state === 'waiting' ? (
              <StatusPanel
                variant="info"
                icon={<span>·</span>}
                headline="auth function in flight"
                detail="the upstream engine connection is not open yet."
              />
            ) : state === 'admitted' ? (
              <StatusPanel
                variant="success"
                icon={<span>✓</span>}
                headline="connection admitted"
                detail="ProxySession derived; upstream websocket opened; frames pumping."
              />
            ) : (
              <StatusPanel
                variant="alert"
                icon={<span>✗</span>}
                headline="upgrade rejected"
                detail="error frame, then close. no WorkerRegistered, no upstream opened."
              />
            )}

            {state === 'rejected' ? (
              <CodeBlock title="out-of-band rejection frame, then close">
                <M>{'{ '}</M>
                <K>&quot;type&quot;</K>: <S>&quot;error&quot;</S>
                {',\n  '}
                <K>&quot;error&quot;</K>: <M>{'{ '}</M>
                <K>&quot;code&quot;</K>: <S>&quot;AUTH_ERROR&quot;</S>
                {', '}
                <K>&quot;message&quot;</K>: <S>&quot;...&quot;</S>
                <M>{' } }'}</M>
                {'\n'}
                <C>{'// → WS Close. upstream never dialed.'}</C>
              </CodeBlock>
            ) : null}
          </div>
        </div>

        <div className="border border-rule bg-bg">
          <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
            the same rule, everywhere a policy fn runs
          </div>
          <div className="flex flex-col">
            {FAIL_RULES.map((r) => (
              <div key={r.fn} className="px-4 py-3.5 border-b border-rule-2 last:border-b-0">
                <div className="flex flex-wrap items-baseline gap-x-2">
                  <span className="font-mono text-[13px] font-semibold text-ink">{r.fn}</span>
                  <span className="font-mono text-[11.5px] text-ink-ghost lowercase">{r.when}</span>
                </div>
                <div className="mt-1 font-mono text-[12.5px] leading-[1.6] text-alert lowercase">{r.then}</div>
              </div>
            ))}
            <div className="px-4 py-3.5 font-mono text-[12px] leading-[1.7] text-ink-faint lowercase">
              the only safe default for a security boundary: a broken policy function denies, it does not open the door.
            </div>
          </div>
        </div>
      </div>
    </Section>
  )
}
