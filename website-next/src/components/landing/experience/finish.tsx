"use client"

import { ArrowUpRightIcon } from "@/components/site/iconly"
import { buttonVariants } from "@/components/ui/button-variants"
import { trackCta } from "@/lib/analytics"
import { links } from "@/lib/site"
import { cn } from "@/lib/utils"

/** End-of-flow call to action. */
export function Finish() {
  return (
    <div className="rounded-[12px] bg-gray-3 px-5 py-6 text-center">
      <p className="text-[15px] font-medium text-gray-12">Try it for yourself</p>
      <p className="mt-1 text-[13px] text-gray-10">A worker, a router, state, HTTP and traces, in one system.</p>
      <a
        href={links.quickstart}
        target="_blank"
        rel="noopener noreferrer"
        className={cn(buttonVariants(), "mt-4")}
        onClick={() =>
          trackCta("quickstart", "experience_finish", { cta_label: "quickstart guide", cta_href: links.quickstart })
        }
      >
        Read the quickstart guide
        <ArrowUpRightIcon className="size-3.5 text-gray-1/70" />
      </a>
    </div>
  )
}
