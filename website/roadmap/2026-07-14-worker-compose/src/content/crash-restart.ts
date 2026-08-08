/* crash & restart — the daemon's durable ownership record (A16). data only. */

import type { TimelineStage } from '@lib/components/diagrams/DurabilityTimeline'

export const CRASH_STAGES: TimelineStage[] = [
  {
    id: 'running',
    label: 'running',
    sub: 'steady state',
    tone: 'ink',
    title: 'every child has a durable record',
    desc: 'the daemon persists pid, process birth identity, process-group id, and log paths per child — before anything can go wrong.',
    record: { status: 'ready', step: 'supervise', calls: 'database · api', note: 'state.json: pid + birth identity + group' },
  },
  {
    id: 'crash',
    label: 'daemon dies',
    sub: 'kill -9',
    tone: 'alert',
    title: 'children outlive the daemon',
    desc: 'a sudden daemon crash stops nothing else. the children keep serving; the state file keeps the ownership claim.',
    record: { status: 'unsupervised', step: 'children alive', calls: 'database · api', note: 'engine connection gone · state on disk' },
    gapAfter: true,
  },
  {
    id: 'restart',
    label: 'restart',
    sub: 'same id, same file',
    tone: 'ink',
    title: 'verify before touching anything',
    desc: 'on start the daemon reads each record and verifies the live process against its birth identity. a recycled pid is never signaled.',
    record: { status: 'reconciling', step: 'verify identities', calls: 'pid 48213 ✓ · pid 48214 ?', note: 'unverifiable → report, never kill' },
  },
  {
    id: 'reconcile',
    label: 'reconcile',
    sub: 'live child',
    tone: 'accent',
    title: 'live and valid → managed again',
    desc: 'a verified child is re-adopted: claim reconciled with the engine, supervision resumes, no restart needed.',
    record: { status: 'ready', step: 'adopted', calls: 'database · api', note: 'zero downtime through the daemon crash' },
  },
  {
    id: 'cleanup',
    label: 'cleanup',
    sub: 'dead child',
    tone: 'warn',
    title: 'dead → clean, cascade if needed',
    desc: 'a child that died while the daemon was down is detected here: stale state removed, registration released, its post_run fired, local dependents cascaded.',
    record: { status: 'failed', step: 'cascade dependents', calls: 'api stopped', note: 'post_run fired · cause in the logs' },
  },
]
