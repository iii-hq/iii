import { AnywhereExplorer } from '@/components/graphics/anywhere-explorer'
import { Reveal } from '@/components/site/reveal'
import { Section } from '@/components/site/section'

const platforms: { os: string; variants?: string[] }[] = [
  { os: 'macOS' },
  { os: 'Linux', variants: ['glibc', 'musl', 'x86', 'ARM', '32-bit ARM'] },
  { os: 'Windows' },
]

export function Anywhere() {
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: This homepage section has one stable public anchor.
    <Section
      id="anywhere"
      eyebrow="Runs anywhere"
      title="If it can open a WebSocket, it can be a worker."
      lede="The worker protocol is JSON over WebSocket, and it's documented. SDKs exist for Node, Python, Rust, Go, and the browser; another language needs about a dozen message types to become a first-class worker."
    >
      <AnywhereExplorer />
      <Reveal delay={0.1} className="mt-4">
        <ul
          aria-label="Supported platforms"
          className="flex flex-wrap items-center gap-x-6 gap-y-3 px-1 font-mono text-[12px] text-muted-foreground"
        >
          {platforms.map((p) => (
            <li key={p.os} className="flex flex-wrap items-center gap-x-2 gap-y-1.5">
              <span className="text-foreground">{p.os}</span>
              {p.variants?.map((v) => (
                <span key={v} className="rounded-md border px-1.5 py-0.5 text-[12px]">
                  {v}
                </span>
              ))}
            </li>
          ))}
          <li className="ml-auto">
            The <code className="text-foreground">bridge</code> worker links engines on different machines or networks.
          </li>
        </ul>
      </Reveal>
    </Section>
  )
}
