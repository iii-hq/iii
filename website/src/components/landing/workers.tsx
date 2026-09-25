import { SectionIntro } from "@/components/landing/section-intro"
import { ArrowUpRightIcon } from "@/components/site/iconly"
import { TrackedLink } from "@/components/site/tracked-link"
import { buttonVariants } from "@/components/ui/button-variants"
import { tokenize } from "@/lib/shiki"
import { links } from "@/lib/site"
import { cn } from "@/lib/utils"
import { addCommand, registersLine, WORKERS, type WorkerName } from "./workers/catalog"
import type { CardCode } from "./workers/worker-card"
import { WorkersShowcase } from "./workers/workers-showcase"

/** Every card's `$` and `›` lines, highlighted once on the server (bash / TypeScript). */
async function tokenizeCards(): Promise<Record<WorkerName, CardCode>> {
  const entries = await Promise.all(
    WORKERS.map(async (w) => {
      const [add, trigger] = await Promise.all([tokenize(addCommand(w), "bash"), tokenize(registersLine(w), "bash")])
      return [w.name, { add: add[0] ?? [], trigger }] as const
    }),
  )
  return Object.fromEntries(entries) as Record<WorkerName, CardCode>
}

/**
 * "Any service, one abstraction": a registry search types what you'd look
 * for, the strip of worker cards glides to the real worker and its install
 * command types itself in. Below it, the link out to the registry itself.
 */
export async function Workers() {
  const code = await tokenizeCards()

  return (
    // biome-ignore lint/correctness/useUniqueElementIds: the header's section rail and #workers anchors target this id
    <section id="workers" aria-labelledby="workers-title" className="scroll-mt-[116px] py-20 sm:py-24">
      <div className="mx-auto max-w-[1200px] px-5 md:px-6">
        <SectionIntro eyebrow="Workers" titleId="workers-title" title="Any service, one abstraction.">
          The answer to “we need X” stops being “evaluate, procure, integrate.” It becomes “add a worker.”
        </SectionIntro>

        <WorkersShowcase code={code} className="mt-10 sm:mt-12" />

        {/* `nw-cta-row` is scraped by website/scripts/generate-llms-agents.ts (`#workers .nw-cta-row`); keep it. */}
        <div className="nw-cta-row mt-8 flex justify-end">
          <TrackedLink
            href={links.workerRegistry}
            target="_blank"
            rel="noopener noreferrer"
            cta={{ cta_id: "worker_registry", cta_location: "workers_section", cta_label: "view the worker registry" }}
            className={cn(buttonVariants({ variant: "outline" }), "group/cta max-sm:w-full")}
          >
            View the worker registry
            <ArrowUpRightIcon className="size-4 text-gray-10 transition-transform duration-200 ease-out group-hover/cta:translate-x-px group-hover/cta:-translate-y-px" />
          </TrackedLink>
        </div>
      </div>
    </section>
  )
}
