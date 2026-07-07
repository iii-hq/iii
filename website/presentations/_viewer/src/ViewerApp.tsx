import { Sheet } from '@lib/components/schematic/Sheet'
import { TopNav } from '@lib/components/TopNav'
import { useHashRoute } from '@lib/hooks/useHashRoute'
import { SpecPage } from '@lib/pages/SpecPage'
import { useEffect, useState } from 'react'

/**
 * The generic markdown spec viewer — the page a spec gets at
 * /roadmap/<slug>/ before (or without) an interactive deck. build.mjs
 * copies this app per md-only spec and drops a spec.json beside it; the raw
 * `.md` files sit in the same directory, so everything is fetched relative.
 */

interface SpecManifest {
  slug: string
  title: string
  tagline: string
  docs: { file: string; label: string }[]
}

type LoadState =
  | { kind: 'loading' }
  | { kind: 'error' }
  | { kind: 'ready'; manifest: SpecManifest; docs: Record<string, string> }

export function ViewerApp() {
  const route = useHashRoute()
  const [state, setState] = useState<LoadState>({ kind: 'loading' })

  useEffect(() => {
    let cancelled = false
    const load = async () => {
      try {
        const res = await fetch('./spec.json')
        if (!res.ok) throw new Error(String(res.status))
        const manifest = (await res.json()) as SpecManifest
        const entries = await Promise.all(
          manifest.docs.map(async ({ file }) => {
            const md = await fetch(`./${file}`)
            if (!md.ok) throw new Error(`${file}: ${md.status}`)
            return [file, await md.text()] as const
          }),
        )
        if (cancelled) return
        document.title = `iii — ${manifest.title}`
        setState({ kind: 'ready', manifest, docs: Object.fromEntries(entries) })
      } catch {
        if (!cancelled) setState({ kind: 'error' })
      }
    }
    void load()
    return () => {
      cancelled = true
    }
  }, [])

  return (
    <div className="@container min-h-screen">
      <Sheet>
        <TopNav
          route={route}
          meta={{
            wordmarkLabel: state.kind === 'ready' ? state.manifest.title : 'iii / tech-specs',
          }}
          nav={[]}
          specHref={null}
        />
        {state.kind === 'loading' ? (
          <main className="px-4 py-24 @3xl:px-9">
            <p className="font-mono text-[14px] lowercase text-ink-faint">loading spec…</p>
          </main>
        ) : state.kind === 'error' ? (
          <main className="px-4 py-24 @3xl:px-9">
            <p className="font-mono text-[14px] lowercase text-ink-faint">
              this spec could not be loaded.{' '}
              <a href="../" className="text-ink hover:text-accent">
                back to all tech specs
              </a>
            </p>
          </main>
        ) : (
          <>
            <section className="border-b border-rule px-4 py-10 @3xl:px-9 @3xl:py-12">
              <h1 className="font-mono text-[26px] @3xl:text-[36px] font-semibold lowercase leading-[1.1] tracking-[-0.02em] text-ink max-w-[26ch]">
                {state.manifest.title}
              </h1>
              {state.manifest.tagline ? (
                <p className="mt-4 font-mono text-[13.5px] leading-[1.7] text-ink-faint max-w-[70ch]">
                  {state.manifest.tagline}
                </p>
              ) : null}
              <p className="mt-4 font-mono text-[11px] uppercase tracking-[0.08em] text-ink-ghost">
                spec — markdown edition ·{' '}
                <a href="../" className="normal-case lowercase text-ink-faint hover:text-ink">
                  all tech specs →
                </a>
              </p>
            </section>
            <SpecPage docs={state.docs} />
          </>
        )}
      </Sheet>
    </div>
  )
}
