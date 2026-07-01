import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { C, CodeBlock, M, S } from '@lib/components/schematic/CodeBlock'
import { FnChip } from '@lib/components/schematic/FnChip'
import { ModeToggle } from '@lib/components/schematic/ModeToggle'
import { useMemo, useState } from 'react'
import { ACCESS_CATALOG, ACCESS_PROFILES, filterLabel, resolveAccess } from '../content/access'

/**
 * A9 — the access-resolution explorer. Switch the session profile and every
 * function id is re-evaluated live by the exact five-rule order the proxy
 * vendors. Click a function to see which rule decided it.
 */
export function AccessSection() {
  const [profileId, setProfileId] = useState(ACCESS_PROFILES[0].id)
  const [picked, setPicked] = useState<string | null>(null)

  const profile = ACCESS_PROFILES.find((p) => p.id === profileId) ?? ACCESS_PROFILES[0]

  const verdicts = useMemo(() => ACCESS_CATALOG.map((fn) => ({ fn, v: resolveAccess(fn, profile) })), [profile])
  const callable = verdicts.filter((x) => x.v.allowed).length
  const pickedVerdict = picked ? verdicts.find((x) => x.fn.id === picked) : undefined

  const list = (ids: string[]) =>
    ids.length === 0 ? (
      <M>[]</M>
    ) : (
      <>
        <M>[</M>
        <S>{ids.join(', ')}</S>
        <M>]</M>
      </>
    )

  return (
    <Section
      id="access"
      index="04"
      eyebrow="access resolution"
      title="five rules decide every call, and the proxy runs them."
      lede="a worker over websocket gets no in-process session, so the engine cannot filter for it. the proxy derives each connection's boundaries and resolves every invoke itself. switch the profile to watch the surface change."
    >
      <div className="border border-rule bg-bg">
        <div className="flex flex-wrap items-center justify-between gap-3 bg-panel px-3.5 py-2.5 border-b border-rule">
          <ModeToggle
            value={profileId}
            onChange={(v) => setProfileId(v)}
            options={ACCESS_PROFILES.map((p) => ({ value: p.id, label: p.label }))}
          />
          <span className="font-mono text-[12px] uppercase tracking-[0.06em] text-ink-faint">
            <span className="text-accent text-[15px] font-semibold tabular-nums">{callable}</span> /{' '}
            {ACCESS_CATALOG.length} callable
          </span>
        </div>

        <div className="flex flex-wrap gap-2 p-4">
          {verdicts.map(({ fn, v }) => {
            const isPicked = picked === fn.id
            const tone = isPicked ? 'accent' : v.allowed ? 'ink' : 'ghost'
            return (
              <button
                key={fn.id}
                type="button"
                aria-pressed={isPicked}
                onClick={() => setPicked(isPicked ? null : fn.id)}
                title={`${v.allowed ? 'allowed' : 'denied'} · ${v.rule}`}
                className="cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
              >
                <FnChip tone={tone} className={v.allowed ? '' : 'line-through opacity-55 decoration-alert/70'}>
                  {fn.id}
                </FnChip>
              </button>
            )
          })}
        </div>

        <div className="border-t border-rule px-4 py-3 min-h-[58px] flex items-center">
          {pickedVerdict ? (
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
              <FnChip tone={pickedVerdict.v.allowed ? 'accent' : 'alert'}>{pickedVerdict.fn.id}</FnChip>
              <span
                className={`font-mono text-[13px] font-semibold lowercase ${
                  pickedVerdict.v.allowed ? 'text-accent' : 'text-alert'
                }`}
              >
                {pickedVerdict.v.allowed ? 'allowed' : 'denied'}
              </span>
              <span className="font-mono text-[12px] text-ink-faint">{pickedVerdict.v.rule}</span>
            </div>
          ) : (
            <p className="font-mono text-[12.5px] leading-[1.6] text-ink-faint lowercase">
              {profile.note} click a function to see which rule decides it.
            </p>
          )}
        </div>
      </div>

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-4">
        <CodeBlock title="active policy · configuration: rbac-proxy">
          <M>rbac:</M>
          {'\n  forbidden_functions: '}
          {list(profile.forbidden_functions)}
          {'\n  allowed_functions: '}
          {list(profile.allowed_functions)}
          {'\n  expose_functions:'}
          {profile.expose.length === 0 ? <M> []</M> : null}
          {profile.expose.map((f, i) => (
            <span key={i}>
              {'\n    - '}
              <S>{filterLabel(f)}</S>
            </span>
          ))}
          {'\n'}
          <C>{'# carve-out (10 ids) stays callable regardless'}</C>
        </CodeBlock>

        <SpecSheet title="the resolution order" meta="forbidden wins" defaultOpen>
          <div className="flex flex-col">
            <SpecRow name="1 · forbidden_functions" type="deny">
              the hard deny. a per-user or per-role denylist expose cannot override.
            </SpecRow>
            <SpecRow name="2 · allowed_functions" type="allow">
              an explicit allow from the auth result, beyond expose.
            </SpecRow>
            <SpecRow name="3 · carve-out" type="allow">
              the ten infrastructure ids: channels, logging, baggage, register.
            </SpecRow>
            <SpecRow name="4 · expose_functions" type="allow">
              any wildcard or metadata filter matches.
            </SpecRow>
            <SpecRow name="5 · otherwise" type="deny">
              an empty expose is not deny-everything; it is allow only the carve-out plus whatever the auth result
              allowed.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}
