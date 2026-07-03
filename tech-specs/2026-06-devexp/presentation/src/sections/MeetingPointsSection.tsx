import { Section } from '@/components/Section';
import { SpecSheet, SpecRow } from '@/components/SpecSheet';
import { MeetingPointsSequence } from '@/components/diagrams/MeetingPointsSequence';
import { MEETING_POINTS } from '@/content/map';

export function MeetingPointsSection() {
  return (
    <Section
      id="meeting-points"
      index="04"
      eyebrow="the contact surface"
      title="the only three places the planes touch."
      lede="during an up: ops calls the daemon ①, the daemon calls sandbox ②, the worker connects back ③. play the sequence for one node coming up."
    >
      {/* 1) the live sequence */}
      <MeetingPointsSequence />

      {/* 2) the three contact points */}
      <div className="mt-6 grid @3xl:grid-cols-3 gap-px bg-rule border border-rule">
        {MEETING_POINTS.map((mp) => (
          <div key={mp.badge} className="bg-bg p-5">
            <div className="flex h-6 w-6 items-center justify-center border border-accent font-mono text-[12px] tabular-nums text-accent">
              {mp.badge}
            </div>
            <div className="mt-3 font-mono text-[11px] text-ink-faint lowercase">{mp.from}</div>
            <div className="mt-1 font-mono text-[14px] font-semibold lowercase text-ink">
              {mp.title}
            </div>
            <div className="mt-2 text-[12px] text-ink-faint lowercase">{mp.detail}</div>
          </div>
        ))}
      </div>

      {/* 3) the correlation key */}
      <div className="mt-8">
        <SpecSheet title="the correlation key" meta="(port, compose_id, instance_token)">
          <SpecRow name="instance_token" type="daemon-minted, III_INSTANCE_TOKEN">
            joins process identity (the daemon&apos;s table) to connection identity (the engine
            registry).
          </SpecRow>
          <SpecRow name="managed" type="token present">
            full control — the daemon is the parent, reaps, killpg, restarts per policy.
          </SpecRow>
          <SpecRow name="observe-only" type="no token">
            a dev-started worker; known via worker_connected, never claimed-reaped.
          </SpecRow>
          <SpecRow name="registration-collision" type="rejected">
            an untokened connection that registers a compose_id already owned by a managed instance
            is rejected and logged.
          </SpecRow>
        </SpecSheet>
      </div>
    </Section>
  );
}
