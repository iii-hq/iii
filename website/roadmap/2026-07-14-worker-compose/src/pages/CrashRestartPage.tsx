import { DurabilityTimeline } from '@lib/components/diagrams/DurabilityTimeline'
import { PageShell } from '@lib/components/PageShell'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { CRASH_STAGES } from '../content/crash-restart'

/**
 * A14 deep dive — the daemon's own crash. Children outlive it; a durable
 * per-child record (pid + birth identity + group) makes restart reconciliation
 * safe: verify, re-adopt the living, clean the dead, never signal a pid it
 * cannot prove is the same process.
 */
export function CrashRestartPage() {
  return (
    <PageShell
      eyebrow="deep dive"
      title="when the daemon itself dies"
      description="an intentional shutdown performs a local down. a sudden crash stops nothing else — the children keep serving, and the state file keeps enough identity to reconcile safely on restart."
    >
      <DurabilityTimeline
        heading="the ownership record through a daemon crash"
        headingNote="kill -9 on the daemon · children alive"
        recordHeading="per-child record"
        stages={CRASH_STAGES}
      />

      <div className="mt-8">
        <SpecSheet title="the invariants" meta="restart reconciliation">
          <SpecRow name="engine disconnect ≠ teardown">
            losing the engine connection never stops children; the daemon reconnects and reconciles.
          </SpecRow>
          <SpecRow name="verify before signal">
            a recorded pid is signaled only after its birth identity matches — a recycled pid is reported, never
            killed.
          </SpecRow>
          <SpecRow name="dead child → full accounting">
            stale state removed, registration released, local dependents cascaded, cause logged.
          </SpecRow>
          <SpecRow name="unverifiable → hands off">
            anything the daemon cannot prove it owns is reported for manual cleanup instead of guessed at.
          </SpecRow>
        </SpecSheet>
      </div>
    </PageShell>
  )
}
