"use client"

import { Swap } from "@/components/motion/swap"
import { CheckIcon, CopyIcon } from "@/components/site/iconly"
import { useCopy } from "@/hooks/use-copy"
import { trackCta } from "@/lib/analytics"
import { INSTALL_COMMAND } from "@/lib/site"
import { cn } from "@/lib/utils"

/** What the box shows. The clipboard gets the full command, protocol included. */
const INSTALL_DISPLAY = INSTALL_COMMAND.replace("https://", "")
/** The compact form keeps both ends (the domain and the `| sh`) and elides the path. */
const INSTALL_COMPACT = INSTALL_DISPLAY.replace(/(install\.iii\.dev)\/\S+/, "$1/…")

type InstallCommandProps = {
  className?: string
  /** analytics `cta_location` */
  location?: "hero" | "footer"
  /** one 44px line that fits beside a button: path elided, icon-only copy chip, full command on hover */
  compact?: boolean
}

/**
 * The install one-liner, in the hero and on the get-started card. The whole box
 * is the copy button (a bigger target than a glyph alone); a small "Copy" chip at the end
 * names the action and flips to "Copied" for two seconds. The command is
 * always shown in full: it wraps onto a second line when the box is narrow.
 */
export function InstallCommand({ className, location = "footer", compact = false }: InstallCommandProps) {
  const { copied, copy } = useCopy(2000)

  return (
    <>
      <button
        type="button"
        aria-label="Copy install command"
        title={compact ? INSTALL_COMMAND : undefined}
        onClick={() => {
          trackCta("install", location, { cta_label: "install.sh" })
          void copy(INSTALL_COMMAND)
        }}
        className={cn(
          "group/install flex w-full min-w-0 cursor-pointer items-center gap-3 bg-gray-1 pr-2.5 pl-4 text-left font-mono text-[14px] leading-[1.6] tracking-[0.01em] text-gray-12 shadow-[inset_0_0_0_1px_var(--gray-6)] outline-none transition-[background-color,box-shadow] duration-150 ease-out hover:shadow-[inset_0_0_0_1px_var(--gray-7)] focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-2",
          compact ? "h-11 rounded-[10px] whitespace-nowrap max-sm:text-[13px]" : "min-h-12 rounded-[12px] py-2.5",
          className,
        )}
      >
        <span
          aria-hidden="true"
          className={cn("shrink-0 text-gray-9 select-none max-sm:hidden", !compact && "self-start")}
        >
          $
        </span>
        {compact ? (
          <span className="min-w-0 flex-1">{INSTALL_COMPACT}</span>
        ) : (
          <>
            {/* Phones: the elided command, broken once after the flags so it reads as two clean lines. */}
            <span className="min-w-0 flex-1 text-[13px] sm:hidden">
              curl -fsSL
              <br />
              {INSTALL_COMPACT.replace("curl -fsSL ", "")}
            </span>
            <span className="min-w-0 flex-1 max-sm:hidden [overflow-wrap:anywhere]">{INSTALL_DISPLAY}</span>
          </>
        )}
        <span
          aria-hidden="true"
          className={cn(
            "inline-flex h-7 shrink-0 items-center gap-1.5 rounded-[7px] bg-gray-3 font-sans text-[12px] font-medium tracking-normal text-gray-11 shadow-[inset_0_0_0_1px_var(--gray-6)]",
            // Phones drop the label so the command keeps two lines; wider keeps "Copy" / "Copied".
            compact ? "w-7 justify-center" : "px-2 max-sm:w-7 max-sm:justify-center max-sm:px-0",
            "transition-[background-color,color] duration-150 ease-out group-hover/install:bg-gray-4 group-hover/install:text-gray-12",
            copied && "text-gray-12",
          )}
        >
          <Swap id={copied ? "copied" : "idle"}>
            {copied ? (
              <>
                <CheckIcon className="size-3.5" />
                {!compact && <span className="max-sm:hidden">Copied</span>}
              </>
            ) : (
              <>
                <CopyIcon className="size-3.5" />
                {!compact && <span className="max-sm:hidden">Copy</span>}
              </>
            )}
          </Swap>
        </span>
      </button>
      <output className="sr-only" aria-live="polite">
        {copied ? "Install command copied to clipboard" : ""}
      </output>
    </>
  )
}
