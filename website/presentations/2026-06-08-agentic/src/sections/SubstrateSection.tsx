import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { C, CodeBlock, K, M, S } from '@lib/components/schematic/CodeBlock'
import { FnChip } from '@lib/components/schematic/FnChip'
import { ModeToggle } from '@lib/components/schematic/ModeToggle'
import { StatusPanel } from '@lib/components/schematic/StatusPanel'
import { useMemo, useState } from 'react'

const REGISTRY = [
  'shell::exec',
  'shell::spawn',
  'todo::create',
  'todo::list',
  'email::send',
  'database::query',
  'storage::put',
  'storage::get',
  'image-resize::convert',
  'search::web',
  'coder::apply_patch',
  'harness::spawn',
  'session::messages',
  'session::delete',
  'router::models::list',
  'engine::functions::list',
  'engine::workers::list',
  'telegram::send',
] as const

type PolicyId = 'default' | 'narrow' | 'broad'

const POLICIES: Record<PolicyId, { allow: string[]; deny: string[]; caption: string }> = {
  default: {
    allow: [],
    deny: [],
    caption:
      'no allow-list supplied — every call is refused with a result the model can read. a default install is a plain chat loop until you opt functions in.',
  },
  narrow: {
    allow: ['todo::*', 'search::web', 'engine::functions::list'],
    deny: [],
    caption:
      'a narrow agent: three globs, nothing else reachable. the same registry, a different reach — policy is per turn, not per deployment.',
  },
  broad: {
    allow: ['*'],
    deny: ['session::delete'],
    caption:
      'a broad agent: everything allowed except destructive surfaces — the shape a deployment pairs with approval-gate holds on the risky calls.',
  },
}

function globToRegex(glob: string): RegExp {
  const escaped = glob.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*')
  return new RegExp(`^${escaped}$`)
}

function isAllowed(fn: string, policy: { allow: string[]; deny: string[] }) {
  const allowed = policy.allow.some((g) => globToRegex(g).test(fn))
  const denied = policy.deny.some((g) => globToRegex(g).test(fn))
  return allowed && !denied
}

export function SubstrateSection() {
  const [policyId, setPolicyId] = useState<PolicyId>('broad')
  const policy = POLICIES[policyId]
  const allowedCount = useMemo(() => REGISTRY.filter((fn) => isAllowed(fn, policy)).length, [policy])

  return (
    <Section
      id="substrate"
      index="04"
      eyebrow="the white box"
      title="everything callable is a function — the agent's world is the live registry."
      lede="there is no separate capability layer bolted onto the loop. whatever workers register on the bus — shell, email, storage, your own domain functions — an agent can be allowed to reach through one invocation surface. it discovers what exists at runtime; policy decides what it may touch."
    >
      <div className="grid grid-cols-1 @4xl:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] gap-6 items-start">
        <div className="flex flex-col gap-4 min-w-0">
          <CodeBlock title="the model sees one surface">
            <K>agent_trigger</K> <M>{'{'}</M>
            {'\n'}
            {'  '}function: <S>"shell::exec"</S>,{'\n'}
            {'  '}payload: <M>{'{'}</M> command: <S>"ls -la"</S> <M>{'}'}</M>
            {'\n'}
            <M>{'}'}</M>
            {'\n\n'}
            <C>// discovery is runtime, not setup:</C>
            {'\n'}
            <K>agent_trigger</K> <M>{'{'}</M> function: <S>"engine::functions::list"</S> <M>{'}'}</M>
          </CodeBlock>
          <StatusPanel
            variant="info"
            headline="the harness understands the whole ecosystem"
            detail="registry reads pass through the same allow/deny globs — the model only ever discovers functions it can actually call. a new worker connects, and every agent that is allowed to see it, sees it."
          />
          <StatusPanel
            variant="success"
            headline="fail closed, by spec"
            detail="dispatch policy is structural and final: no allow-list means no calls. hooks can narrow it further, never bypass it."
          />
        </div>

        <div className="border border-rule bg-bg min-w-0">
          <div className="flex flex-wrap items-center justify-between gap-3 bg-panel px-3.5 py-2.5 border-b border-rule">
            <span className="font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
              dispatch policy — try one
            </span>
            <ModeToggle
              value={policyId}
              onChange={setPolicyId}
              options={[
                { value: 'default', label: 'default' },
                { value: 'narrow', label: 'narrow' },
                { value: 'broad', label: 'broad' },
              ]}
            />
          </div>

          <div className="px-4 py-3 border-b border-rule-2 font-mono text-[12.5px] leading-[1.6]">
            <span className="text-ink-faint">functions: </span>
            <span className="text-ink">{'{'}</span> allow:{' '}
            <span className="text-accent">[{policy.allow.map((g) => `"${g}"`).join(', ')}]</span>
            {policy.deny.length > 0 ? (
              <>
                , deny: <span className="text-ink">[{policy.deny.map((g) => `"${g}"`).join(', ')}]</span>
              </>
            ) : null}{' '}
            <span className="text-ink">{'}'}</span>
          </div>

          <div className="px-4 py-4">
            <div className="flex flex-wrap gap-2">
              {REGISTRY.map((fn) => {
                const ok = isAllowed(fn, policy)
                return (
                  <FnChip
                    key={fn}
                    tone={ok ? 'ink' : 'ghost'}
                    className={ok ? '' : 'opacity-55 line-through decoration-rule'}
                  >
                    {fn}
                  </FnChip>
                )
              })}
            </div>
            <p className="mt-4 font-mono text-[12px] leading-[1.65] text-ink-faint lowercase max-w-[64ch]">
              {policy.caption}
            </p>
          </div>

          <div className="flex items-center justify-between bg-panel border-t border-rule px-3.5 py-2">
            <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint">reachable</span>
            <span className="font-mono text-[12px] text-ink tabular-nums">
              {allowedCount}/{REGISTRY.length}
            </span>
          </div>
        </div>
      </div>

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="exposure modes" meta="per turn">
          <div className="flex flex-col">
            <SpecRow name='expose: "agent_trigger"' type="default">
              one generic schema, zero per-function token cost. discovery happens at runtime through the registry —
              right for broad, exploratory agents.
            </SpecRow>
            <SpecRow name='expose: "native"' type="narrow agents">
              the harness expands the allow globs against the registry at turn start and attaches one concrete schema
              per function — models follow exact schemas more reliably, the right mode for sub-agents with small fixed
              surfaces.
            </SpecRow>
            <SpecRow name="same policy either way" type="invariant">
              exposure changes what the model sees, never what it may call — the fail-closed globs run at dispatch time
              in both modes.
            </SpecRow>
          </div>
        </SpecSheet>

        <SpecSheet title="provenance — calls cannot be laundered" meta="security">
          <div className="flex flex-col">
            <SpecRow name="agent provenance" type="propagated">
              every call made on behalf of a model carries a mark, and the engine propagates it through nested calls — a
              function the agent reaches cannot re-trigger its way around policy.
            </SpecRow>
            <SpecRow name="permission rules" type="deployment-level">
              rules apply to any call whose chain carries the mark; worker-initiated and human-initiated calls pass
              untouched.
            </SpecRow>
            <SpecRow name="sub-agents inherit" type="narrow, never escalate">
              a child's policy is the parent's intersected with the request — depth, fan-out, and turn budgets bound the
              whole tree.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
