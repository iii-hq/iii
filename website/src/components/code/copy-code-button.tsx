"use client"

import { Swap } from "@/components/motion/swap"
import { CheckIcon, CopyIcon } from "@/components/site/iconly"
import { buttonVariants } from "@/components/ui/button-variants"
import { useCopy } from "@/hooks/use-copy"
import { cn } from "@/lib/utils"

export function CopyCodeButton({ code, className }: { code: string; className?: string }) {
  const { copied, copy } = useCopy(2000)
  return (
    <button
      type="button"
      aria-label={copied ? "Copied" : "Copy code"}
      onClick={() => void copy(code)}
      className={cn(
        buttonVariants({ variant: "ghost", size: "icon-xs" }),
        "text-gray-10 hover:text-gray-12",
        className,
      )}
    >
      <Swap id={copied ? "copied" : "idle"}>
        {copied ? <CheckIcon className="size-3.5" /> : <CopyIcon className="size-3.5" />}
      </Swap>
    </button>
  )
}
