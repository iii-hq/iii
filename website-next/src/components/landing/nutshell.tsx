import { NUTSHELL_GROUPS } from "./nutshell/nutshell-data"
import { SectionIntro } from "./section-intro"

/** "iii in a nutshell." Two trait groups, each a row of three cards under a heading. */
export function Nutshell() {
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: single-instance section; #nutshell anchors link here
    <section id="nutshell" aria-labelledby="nutshell-title" className="scroll-mt-[116px] py-20 sm:py-24">
      <div className="mx-auto max-w-[1200px] px-5 md:px-6">
        <SectionIntro titleId="nutshell-title" title="iii in a nutshell.">
          Every capability, every framework, and every tool become a pattern on the same core system.
        </SectionIntro>

        <div className="mt-10 flex flex-col gap-16 sm:mt-12 sm:gap-20">
          {NUTSHELL_GROUPS.map((group) => {
            return (
              <div key={group.title}>
                <div className="min-w-0">
                  <h3 className="text-[20px] leading-[1.3] font-medium tracking-[-0.02em] text-gray-12">
                    {group.title}
                  </h3>
                  <p className="mt-1.5 max-w-[560px] text-[14px] leading-[1.6] text-pretty text-gray-11">
                    {group.tagline}
                  </p>
                </div>

                <ul className="mt-6 grid gap-4 lg:grid-cols-3">
                  {group.points.map((p) => (
                    <li key={p.title} className="rounded-[14px] bg-gray-2 p-6 shadow-panel">
                      <h4 className="text-[15px] font-medium text-gray-12">{p.title}</h4>
                      <p className="mt-2 max-w-[480px] text-[14px] leading-[1.6] text-pretty text-gray-11">{p.body}</p>
                    </li>
                  ))}
                </ul>
              </div>
            )
          })}
        </div>
      </div>
    </section>
  )
}
