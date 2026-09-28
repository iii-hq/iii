"use client"

import { useId } from "react"
import { InstallCommand } from "@/components/landing/get-started/install-command"
import { Swap } from "@/components/motion/swap"
import { ArrowRightIcon, CheckIcon } from "@/components/site/iconly"
import { buttonVariants } from "@/components/ui/button-variants"
import { useCopy } from "@/hooks/use-copy"
import { trackCta } from "@/lib/analytics"
import { COPY_PROMPT_TEXT, links } from "@/lib/site"
import { cn } from "@/lib/utils"
import { AGENT_MARKS } from "./agent-marks"

const PROMPT_PREVIEW = `${COPY_PROMPT_TEXT.split("\n")[0].replace(/\.$/, "")}…`

/**
 * The hero's actions: the primary button beside the install one-liner (the box
 * is the copy button), then the on-ramp for readers who are using a coding
 * agent.
 */
export function HeroActions() {
  const prompt = useCopy(2000)
  const previewId = useId()

  return (
    <div className="flex w-full flex-col items-start gap-4">
      <div className="flex w-full flex-col gap-3 sm:w-auto sm:flex-row sm:items-center">
        <a
          href={links.install}
          onClick={() => trackCta("get_started", "hero", { cta_label: "get started" })}
          className={cn(buttonVariants({ size: "lg" }), "group/cta shrink-0 sm:pr-4")}
        >
          Get started
          <ArrowRightIcon className="size-4 transition-transform duration-200 ease-out group-hover/cta:translate-x-0.5" />
        </a>
        {/* Path elided so it sits beside the button; the clipboard gets the whole command. */}
        <InstallCommand location="hero" compact className="sm:w-auto focus-visible:ring-offset-gray-1" />
      </div>

      {/* The AI on-ramp: "works with" logos + one quiet action. */}
      <div className="group/prompt relative -ml-2.5">
        <button
          type="button"
          aria-label="Copy the prompt for your coding agent"
          aria-describedby={previewId}
          onClick={() => {
            trackCta("copy_prompt", "hero", { cta_label: "copy prompt" })
            void prompt.copy(COPY_PROMPT_TEXT)
          }}
          className={cn(buttonVariants({ variant: "ghost", size: "sm" }), "gap-2.5 pl-2.5 text-gray-11")}
        >
          <span aria-hidden="true" className="flex items-center">
            {AGENT_MARKS.map((mark) => (
              <span
                key={mark.id}
                title={mark.name}
                className="flex size-6 items-center justify-center rounded-full bg-gray-4 text-gray-12 ring-2 ring-gray-1 not-first:-ml-1.5"
              >
                <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" className="size-3">
                  <path d={mark.d} />
                </svg>
              </span>
            ))}
          </span>
          <Swap id={prompt.copied ? "copied" : "idle"} className="gap-1.5">
            {prompt.copied ? (
              <>
                <CheckIcon className="size-3.5 text-gray-12" />
                <span className="text-gray-12">Prompt copied</span>
              </>
            ) : (
              <>
                Using a coding agent? <span className="text-gray-12">Copy the prompt</span>
              </>
            )}
          </Swap>
        </button>

        <div
          id={previewId}
          role="tooltip"
          className={cn(
            "pointer-events-none absolute top-full left-2.5 z-20 mt-1.5 w-max max-w-[360px] origin-top-left rounded-[10px] bg-gray-3 px-3 py-2 text-left text-[12.5px] leading-[1.5] text-gray-11 opacity-0 shadow-panel",
            "scale-[0.97] transition-[opacity,scale] duration-150 ease-out max-sm:hidden motion-reduce:transition-none",
            "group-focus-within/prompt:scale-100 group-focus-within/prompt:opacity-100 group-hover/prompt:scale-100 group-hover/prompt:opacity-100 group-hover/prompt:delay-150",
          )}
        >
          “{PROMPT_PREVIEW}”
        </div>
      </div>

      <output className="sr-only" aria-live="polite">
        {prompt.copied ? "Prompt copied to clipboard" : ""}
      </output>
    </div>
  )
}
