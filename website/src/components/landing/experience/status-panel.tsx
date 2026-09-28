"use client"

import { useEffect, useEffectEvent, useState } from "react"
import { CheckIcon } from "@/components/site/iconly"
import type { StatusStep } from "./steps"
import { WaitAction } from "./wait-action"

type StatusPanelProps = { step: StatusStep; onAdvance: (label?: string) => void }

/** A one-line system status ("Classifier worker connected"). Greys only; success gets a filled check. */
export function StatusPanel({ step, onAdvance }: StatusPanelProps) {
  const [advanced, setAdvanced] = useState(false)
  const { autoAdvance } = step
  const success = step.variant === "success"

  const autoAdvanceNow = useEffectEvent(() => {
    setAdvanced(true)
    onAdvance()
  })
  useEffect(() => {
    if (!autoAdvance) return
    const t = setTimeout(autoAdvanceNow, autoAdvance)
    return () => clearTimeout(t)
  }, [autoAdvance])

  return (
    <div className="flex items-center gap-3 rounded-[12px] bg-gray-3 px-3.5 py-2.5">
      <span aria-hidden="true" className="flex size-4 shrink-0 items-center justify-center">
        {success ? (
          <span className="flex size-4 items-center justify-center rounded-full bg-gray-12 text-gray-1">
            <CheckIcon className="size-2.5" />
          </span>
        ) : (
          <span className="size-1.5 rounded-full bg-gray-9" />
        )}
      </span>
      <p className="min-w-0 flex-1 text-[13px] leading-[1.5]">
        <span className="font-medium text-gray-12">{step.headline}</span>
        {step.detail && (
          <>
            <span aria-hidden="true" className="text-gray-7">
              {" · "}
            </span>
            <span className="text-gray-10">{step.detail}</span>
          </>
        )}
      </p>
      {!advanced && !autoAdvance && (
        <WaitAction
          label="Continue"
          variant="outline"
          onAction={(l) => {
            setAdvanced(true)
            onAdvance(l?.toLowerCase())
          }}
        />
      )}
    </div>
  )
}
