import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { Button } from '@lib/components/schematic/Button'
import { FnChip } from '@lib/components/schematic/FnChip'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { cn } from '@lib/lib/utils'
import { useState } from 'react'

type Decision = 'pending' | 'approved' | 'denied'

const FLOW = [
  { id: 'hold', label: 'gate holds the call' },
  { id: 'pending', label: 'pending_created fires' },
  { id: 'human', label: 'a human decides' },
  { id: 'resolve', label: 'function::resolve' },
  { id: 'resume', label: 'the turn resumes' },
] as const

export function GovernanceSection() {
  const [decision, setDecision] = useState<Decision>('pending')
  const flowIndex = decision === 'pending' ? 1 : 4

  return (
    <Section
      id="governance"
      index="08"
      eyebrow="governance"
      title="risky calls wait for a human — and the loop never changes."
      lede="approval-gate is the proof that the loop is governable from outside: one synchronous hook holds a call, a pending inbox carries the decision, one function resumes the parked turn. policy lives in the sibling; the harness ships only the mechanics. try it below."
    >
      {/* flow strip */}
      <div className="flex flex-wrap items-center gap-y-2 border border-rule bg-bg px-3.5 py-2.5 mb-4">
        {FLOW.map((stage, i) => (
          <span key={stage.id} className="flex items-center">
            <span
              className={cn(
                'font-mono text-[11px] lowercase px-2 py-1 border transition-colors',
                i <= flowIndex ? 'border-rule text-ink bg-panel' : 'border-rule-2 text-ink-ghost',
                i === flowIndex && 'border-accent text-accent bg-bg',
              )}
            >
              {stage.label}
            </span>
            {i < FLOW.length - 1 ? <span aria-hidden className="mx-2 h-px w-4 bg-rule inline-block" /> : null}
          </span>
        ))}
      </div>

      <div className="grid grid-cols-1 @4xl:grid-cols-3 gap-4 items-stretch">
        {/* the call */}
        <div className="border border-rule bg-bg flex flex-col">
          <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
            1 — the model asks
          </div>
          <pre className="px-4 py-3.5 font-mono text-[12px] leading-[1.6] text-ink overflow-x-auto">
            <code>
              {'agent_trigger {\n  function: '}
              <span className="text-accent">"shell::exec"</span>
              {',\n  payload: {\n    command: '}
              <span className="text-accent">"rm -rf ./build"</span>
              {'\n  }\n}'}
            </code>
          </pre>
          <div className="px-4 pb-4 mt-auto flex flex-col gap-y-1.5">
            <div className="font-mono text-[11.5px] text-ink-faint lowercase">
              <span className="text-ink">policy:</span> allow ["shell::*"] — matched
            </div>
            <div className="font-mono text-[11.5px] text-ink-faint lowercase">
              <span className="text-ink">hook:</span> approval::gate → <span className="text-warn">hold</span> —
              destructive command, needs a human
            </div>
          </div>
        </div>

        {/* the decision */}
        <div
          className={cn(
            'border flex flex-col transition-colors',
            decision === 'pending' ? 'border-warn' : 'border-rule',
            'bg-bg',
          )}
        >
          <div className="flex items-center justify-between bg-panel px-3.5 py-2 border-b border-rule">
            <span className="font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
              2 — the pending inbox
            </span>
            <span className="flex items-center gap-x-1.5">
              <StatusDot tone={decision === 'pending' ? 'warn' : 'accent'} pulse={decision === 'pending'} />
              <span className="font-mono text-[10px] uppercase tracking-[0.06em] text-ink-faint">
                {decision === 'pending' ? 'awaiting decision' : 'resolved'}
              </span>
            </span>
          </div>
          <div className="px-4 py-3.5 flex flex-col gap-y-2">
            <div className="font-mono text-[13px] font-semibold text-ink">shell::exec</div>
            <div className="font-mono text-[11.5px] text-ink-faint lowercase">
              session s_7a1 · turn t_004 · call fc_91
            </div>
            <FnChip tone="faint" className="self-start">
              command: "rm -rf ./build"
            </FnChip>
            <div className="font-mono text-[11px] text-ink-ghost lowercase">
              expires in 30 min — unresolved holds settle as errors, never park forever
            </div>
          </div>
          <div className="px-4 pb-4 mt-auto">
            {decision === 'pending' ? (
              <div className="flex gap-x-2">
                <Button size="sm" onClick={() => setDecision('approved')}>
                  approve
                </Button>
                <Button variant="pill" size="sm" onClick={() => setDecision('denied')}>
                  deny
                </Button>
              </div>
            ) : (
              <div className="flex items-center justify-between gap-x-2">
                <span
                  className={cn(
                    'font-mono text-[12px] lowercase font-semibold',
                    decision === 'approved' ? 'text-accent' : 'text-alert',
                  )}
                >
                  {decision === 'approved' ? 'approved — released for execution' : 'denied — answered as an error'}
                </span>
                <Button variant="ghost" size="sm" onClick={() => setDecision('pending')}>
                  replay
                </Button>
              </div>
            )}
          </div>
        </div>

        {/* the outcome */}
        <div className="border border-rule bg-bg flex flex-col">
          <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
            3 — the parked turn
          </div>
          {decision === 'pending' ? (
            <div className="px-4 py-3.5 flex flex-col gap-y-2.5">
              <div className="font-mono text-[12.5px] text-ink lowercase">turn t_004 — parked</div>
              <p className="font-mono text-[11.5px] leading-[1.65] text-ink-faint lowercase">
                the call checkpointed as pending and the step ended cleanly. no queue entry is held open, nothing times
                out, nothing blocks — durable execution makes waiting free, whether the human answers in seconds or
                tomorrow.
              </p>
            </div>
          ) : decision === 'approved' ? (
            <div className="px-4 py-3.5 flex flex-col gap-y-2.5">
              <div className="font-mono text-[12.5px] text-accent lowercase">released — the harness executes</div>
              <p className="font-mono text-[11.5px] leading-[1.65] text-ink-faint lowercase">
                approval::resolve releases the call back into the dispatch pipeline: remaining hooks run, the function
                executes with the original call's provenance, the result is appended, and the turn re-enqueues. the
                approver never re-implements dispatch.
              </p>
              <pre className="font-mono text-[11px] leading-[1.55] text-ink-faint border border-rule-2 px-3 py-2 overflow-x-auto">
                <code>{'function_result {\n  content: "removed ./build (4.2mb)",\n  is_error: false\n}'}</code>
              </pre>
            </div>
          ) : (
            <div className="px-4 py-3.5 flex flex-col gap-y-2.5">
              <div className="font-mono text-[12.5px] text-alert lowercase">
                delivered as an error — the model adapts
              </div>
              <p className="font-mono text-[11.5px] leading-[1.65] text-ink-faint lowercase">
                the denial is appended as the call's result under the same deterministic id an execution would have
                used. the model reads the reason in-transcript and proposes another way — no crash, no dangling state.
              </p>
              <pre className="font-mono text-[11px] leading-[1.55] text-ink-faint border border-rule-2 px-3 py-2 overflow-x-auto">
                <code>{'function_result {\n  content: "denied: destructive command",\n  is_error: true\n}'}</code>
              </pre>
            </div>
          )}
        </div>
      </div>

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="why this shape wins" meta="policy as a sibling">
          <div className="flex flex-col">
            <SpecRow name="the loop ships mechanics only" type="hold + resolve">
              the harness knows how to park and resume; it never knows who may approve what. swap the policy worker
              without touching the loop.
            </SpecRow>
            <SpecRow name="notify anywhere" type="reactive">
              approval::pending_created is a trigger type — bind a function and route holds to a console inbox, a chat
              channel, a pager.
            </SpecRow>
            <SpecRow name="per-session modes" type="manual / auto / full">
              tighten or relax per conversation; standing "approve always" grants are honoured in every mode.
            </SpecRow>
            <SpecRow name="crashed gate ≠ open gate" type="fail closed">
              a hook that dies or times out resolves by its failure policy — for approval that means deny, never
              wave-through.
            </SpecRow>
          </div>
        </SpecSheet>

        <SpecSheet title="the audit story" meta="who did what">
          <div className="flex flex-col">
            <SpecRow name="the transcript is the trail" type="function_result">
              every held call ends as a result entry — approved, denied, or expired — attributable to its turn and
              session.
            </SpecRow>
            <SpecRow name="hook mutations annotate" type="origin metadata">
              rewrites at the hooks can attach annotations to the entries they touch, so audits see what actually ran.
            </SpecRow>
            <SpecRow name="bindings are enumerable" type="engine registry">
              the effective hook set is a registry read — no hidden middleware, no config drift.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
