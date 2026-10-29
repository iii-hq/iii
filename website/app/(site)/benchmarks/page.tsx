import { ArrowUpRightIcon } from 'lucide-react'
import type { Metadata } from 'next'

import { ChartCard } from '@/components/benchmarks/chart-card'
import { Figures } from '@/components/benchmarks/figures'
import type { Reference } from '@/components/benchmarks/line-chart'
import { LocalBars } from '@/components/benchmarks/local'
import { MicroGroupCard, MicroMore } from '@/components/benchmarks/micro'
import { FloatingProfileTabs, ProfileProvider, ProfileTabs } from '@/components/benchmarks/profile'
import { Chart, SizeTable } from '@/components/benchmarks/sections'
import { wideContainer } from '@/components/landing/section'
import { Footer } from '@/components/site/footer'
import { PixelHeading } from '@/components/site/pixel-heading'
import { Reveal } from '@/components/site/reveal'
import { Section } from '@/components/site/section'
import { SiteHeader } from '@/components/site/site-header'
import data from '@/content/benchmarks/data.json'
import { day } from '@/lib/benchmarks/format'
import {
  bytes,
  count,
  f,
  figures,
  fitsIn,
  initialProfile,
  microGroups,
  micros,
  percent,
  rate,
} from '@/lib/benchmarks/page'
import type { BenchmarksData, Highlights, IndexEntry, Summary } from '@/lib/benchmarks/types'
import * as views from '@/lib/benchmarks/views'
import { pageMetadata } from '@/lib/seo'
import { links } from '@/lib/site'
import { cn } from '@/lib/utils'

const description =
  'How far one iii engine goes: calls per second, observer activations, and memory per worker and per observer, measured every release on machines of fixed size.'

export const metadata: Metadata = pageMetadata({
  title: 'Benchmarks: how far one engine goes',
  description,
  path: '/benchmarks',
})

const bench = data as unknown as BenchmarksData
const source = 'https://github.com/iii-hq/iii'
const rawData = 'https://iii-hq.github.io/iii/loadtest/index.json'

const linkClass =
  'inline-flex items-center gap-0.5 text-foreground underline decoration-line-strong underline-offset-4 transition-colors hover:decoration-foreground'

function Ext({ href, children }: { href: string; children: React.ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noreferrer" className={linkClass}>
      {children}
      <ArrowUpRightIcon aria-hidden className="size-3.5" />
    </a>
  )
}

const paragraph = 'text-pretty text-[15px] text-muted-foreground leading-relaxed'

export default function BenchmarksPage() {
  const run = bench.loadtest?.latest
  return (
    <>
      <SiteHeader />
      <main>{run ? <Benchmarks run={run} /> : <NoData />}</main>
      <Footer container={wideContainer} />
    </>
  )
}

function NoData() {
  return (
    <section className={cn(wideContainer, 'py-40')}>
      <PixelHeading as="h1" className="text-[40px]">
        Benchmarks
      </PixelHeading>
      <p className={cn(paragraph, 'mt-6 max-w-xl')}>The first run has not been published yet.</p>
    </section>
  )
}

function Benchmarks({ run }: { run: Summary }) {
  const sizes = views.profiles(run)
  const vmSizes = sizes.filter((p) => p.kind === 'vm')
  const caps: Reference[] = vmSizes.map((p) => ({
    kind: 'h',
    y: p.mem_mb ?? 0,
    label: `${p.short} cap · ${f(bytes((p.mem_mb ?? 0) * 1024 * 1024))}`,
    series: p.name,
  }))
  const h = run.highlights
  const micro = bench.micro ? microGroups(bench.micro) : null
  const host = sizes.find((p) => p.kind === 'host')
  const scenarios = new Set(run.runs.map((r) => r.scenario)).size

  return (
    <ProfileProvider profiles={sizes} initial={initialProfile(run)}>
      {/* Hero: what this is, which run, and the machine sizes. */}
      <section aria-labelledby="benchmarks-title" className="landing-section relative">
        <div className={cn(wideContainer, 'pt-[clamp(112px,14vw,168px)] pb-12')}>
          <div className="grid gap-10 lg:grid-cols-[minmax(0,7fr)_minmax(0,5fr)] lg:items-end">
            <div className="min-w-0">
              <Reveal>
                <p className="font-medium text-muted-foreground text-xs uppercase leading-none tracking-[0.08em]">
                  Benchmarks
                </p>
              </Reveal>
              {/* biome-ignore lint/correctness/useUniqueElementIds: One page heading; the section is labelled by it. */}
              <PixelHeading
                as="h1"
                id="benchmarks-title"
                className="mt-5 text-[clamp(32px,4vw,56px)] leading-[1.08] tracking-[-0.01em]"
              >
                How far one engine goes
              </PixelHeading>
              <Reveal delay={0.1}>
                <p className="mt-6 max-w-[58ch] text-pretty text-[17px] text-muted-foreground leading-[1.65]">
                  Every release, we put the iii engine alone in virtual machines of fixed size and drive it with real
                  SDK workers until it stops keeping up. These are the results: what each size sustains, what it costs
                  in CPU and memory on the way, and where it runs out.
                </p>
              </Reveal>
            </div>
            <Reveal delay={0.15}>
              <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-xl border bg-border text-[13px]">
                {[
                  ['Engine', `iii ${run.version}`],
                  ['Measured', day(run.date)],
                  ['Machine', run.machine.label ?? `${run.machine.cpus} CPUs`],
                  [
                    'Sizes',
                    `${vmSizes.length} VM${vmSizes.length === 1 ? '' : 's'}${host ? ' and the host' : ''}, ${scenarios} scenarios`,
                  ],
                ].map(([k, v]) => (
                  <div key={k} className="bg-card px-4 py-3.5">
                    <dt className="text-muted-foreground">{k}</dt>
                    <dd className="mt-1 text-pretty text-foreground">{v}</dd>
                  </div>
                ))}
              </dl>
            </Reveal>
          </div>

          <Reveal delay={0.2} className="mt-12">
            {/* biome-ignore lint/correctness/useUniqueElementIds: The floating tabs watch this one anchor. */}
            <div id="profile-anchor" className="flex flex-wrap items-center justify-between gap-4">
              <div>
                <p className="font-medium text-[15px] text-foreground">Pick a machine size</p>
                <p className="mt-1 text-[13px] text-muted-foreground">
                  Every figure and chart below follows it. The other sizes stay in the charts, behind, to compare.
                </p>
              </div>
              <ProfileTabs className="max-w-full overflow-x-auto" />
            </div>
          </Reveal>
          <Reveal delay={0.25} className="mt-6">
            <Figures figures={figures(run)} />
          </Reveal>
          <Reveal delay={0.3}>
            <p className="mt-5 max-w-[760px] text-pretty text-[13px] text-muted-foreground leading-relaxed">
              {run.machine.cpu} · {run.machine.cpus} CPUs. A size of N vCPU · M is the engine with N virtual CPUs and an
              M memory cap, the load generator beside it on {run.machine.load_vcpus} vCPUs of its own;{' '}
              <a href="#method" className={linkClass}>
                how we measure
              </a>
              .
            </p>
          </Reveal>
        </div>
      </section>

      {/* Calls */}
      {/* biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page. */}
      <Section
        id="calls"
        eyebrow="Calls"
        title="Calls through the engine"
        container={wideContainer}
        split
        lede={
          <>
            <p>
              A worker calls a function another worker hosts. The call goes to the engine, the engine routes it to the
              host, and the answer comes back the same way: four WebSocket frames per call.
            </p>
            <p className="mt-3">
              Callers keep more and more calls in flight, from 1 to 2,048. Throughput grows while the engine has CPU to
              spare, up to a peak: the size&rsquo;s capacity.
            </p>
            <SizeTable
              className="mt-6"
              columns={['Peak', 'p99 there', 'CPU per call']}
              rows={sizes.map((p) => {
                const x = h[p.name]
                return {
                  profile: p,
                  cells: [
                    x?.calls_per_sec ? f(rate(x.calls_per_sec)) : '—',
                    x?.calls_p99_us ? f(micros(x.calls_p99_us)) : '—',
                    x?.cpu_us_per_call ? f(micros(x.cpu_us_per_call)) : '—',
                  ],
                }
              })}
            />
          </>
        }
      >
        <div className="mt-10 space-y-6 lg:mt-0">
          <ChartCard
            title="Calls per second, by calls in flight"
            subtitle="64-byte payloads. Each size gets a fresh engine."
            read="Each line climbs while its engine has idle CPU and peaks when it has none; pushed further, the smaller sizes give some of it back. More cores move the peak up while the CPU per call stays about the same, so capacity grows with them."
          >
            <Chart
              series={views.routing(run)}
              x={{ scale: 'log', title: 'Calls in flight', format: 'count' }}
              y={{ scale: 'linear', title: 'Calls per second', format: 'perSec' }}
              label="Calls per second through the engine, by calls in flight, per machine size"
            />
          </ChartCard>
          <ChartCard
            title="p99 latency, by calls in flight"
            read="Past the peak, more calls in flight only wait in line: latency grows with them while throughput does not. With one call in flight, a call takes longer in the VMs than on the host: the same engine, so that is what virtualization costs at low load."
          >
            <Chart
              series={views.routingLatency(run)}
              x={{ scale: 'log', title: 'Calls in flight', format: 'count' }}
              y={{ scale: 'linear', title: 'p99 latency', format: 'micros' }}
              label="p99 latency of calls through the engine, by calls in flight, per machine size"
            />
          </ChartCard>
        </div>
      </Section>

      {/* Observers */}
      {/* biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page. */}
      <Section
        id="observers"
        eyebrow="Observers"
        title="What observing a function costs"
        container={wideContainer}
        split
        lede={
          <>
            <p>
              Every function is observable. Here each called function gains 1 to 64 observers without a filter, so every
              call activates all of them, fire-and-forget, while 64 callers keep calling.
            </p>
            <p className="mt-3">
              An activation is a call the engine makes on its own, and it costs about as much as one. So the engine
              splits the same CPU between the calls and their activations: the more observers per call, the fewer calls
              fit.
            </p>
          </>
        }
      >
        <div className="mt-10 space-y-6 lg:mt-0">
          <ChartCard
            title="Activations per second, by observers per function"
            read="The line holds near the size's capacity whatever the fan-out. When an observer's target cannot keep up, its queue fills and the excess is dropped, counted as OVERFLOW; the tooltip says how much arrived."
          >
            <Chart
              series={views.fanout(run)}
              x={{ scale: 'band', title: 'Observers per function', format: 'count' }}
              y={{ scale: 'linear', title: 'Activations per second', format: 'perSec' }}
              label="Observer activations per second, by observers per function, per machine size"
            />
          </ChartCard>
          <ChartCard
            title="The observed calls, by observers per function"
            read="Calls slow down as observers are added because they share the engine's CPU with the activations, not because an activation waits for anything: the caller has its answer before any observer runs."
          >
            <Chart
              series={views.fanoutCalls(run)}
              x={{ scale: 'band', title: 'Observers per function', format: 'count' }}
              y={{ scale: 'linear', title: 'Calls per second', format: 'perSec' }}
              label="Calls per second of an observed function, by observers per function, per machine size"
            />
          </ChartCard>
        </div>
      </Section>

      {/* Delivery */}
      {/* biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page. */}
      <Section
        id="delivery"
        eyebrow="Delivery"
        title="How much it delivers in full"
        container={wideContainer}
        split
        lede={
          <>
            <p>
              A source like cron or a database calls its event function (
              <code className="font-mono text-[0.9em] text-foreground">cron::on-tick</code>,{' '}
              <code className="font-mono text-[0.9em] text-foreground">database::on-row-changed</code>) for each event,
              and iii delivers each call to the observers whose args and filters match. We offer a steady rate, from 1k
              to 500k events a second over 16 observers, and count what arrives.
            </p>
            <p className="mt-3">
              Up to some rate, everything does. That rate is the size&rsquo;s sustained capacity, and past it the engine
              does not fall over: it delivers what it can and drops the rest, counted.
            </p>
            <SizeTable
              className="mt-6"
              columns={['Sustained', 'Past it']}
              rows={sizes.map((p) => {
                const x = h[p.name]
                return {
                  profile: p,
                  cells: [
                    x?.sustained_activations_per_sec ? f(rate(x.sustained_activations_per_sec)) : '—',
                    x?.saturated_at
                      ? `${percent(x.saturated_delivered ?? 0)} of ${f(rate(x.saturated_at))}`
                      : x?.sustained_activations_per_sec
                        ? 'not reached'
                        : '—',
                  ],
                }
              })}
            />
          </>
        }
      >
        <div className="mt-10 lg:mt-0">
          <ChartCard
            title="Activations delivered, by events offered"
            subtitle="Per second, over 16 observers, on logarithmic scales: a line on the diagonal delivers everything."
            read="On the dashed line, everything offered arrives. Where a size's line leaves it, its engine is full: it keeps delivering at capacity, and the excess is dropped and counted instead of queueing without bound. While the observers' queues are full, the engine stops reading the reporting worker, whose SDK drops what it cannot hold (BUFFER_FULL)."
          >
            <Chart
              series={views.delivery(run)}
              x={{ scale: 'log', title: 'Events offered per second', format: 'perSec' }}
              y={{ scale: 'log', title: 'Activations delivered per second', format: 'perSec' }}
              refs={[{ kind: 'diagonal', label: 'every event delivered' }]}
              label="Observer activations delivered per second, by events offered per second, per machine size"
            />
          </ChartCard>
        </div>
      </Section>

      {/* Filters */}
      {/* biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page. */}
      <Section
        id="filters"
        eyebrow="Filters"
        title="Filters at scale"
        container={wideContainer}
        split
        lede={
          <>
            <p>
              Observers usually filter: a cron expression, a table, a tenant. Here one function has 16 to 4,000
              observers, each filtering on a different value, and every call matches exactly one of them.
            </p>
            <p className="mt-3">
              What grows is the filtering, not the deliveries: the engine checks every observer&rsquo;s filter on every
              call to the function.
            </p>
          </>
        }
      >
        <div className="mt-10 lg:mt-0">
          <ChartCard
            title="Calls per second, by filtered observers on the function"
            read="Each filter check costs about 16 ns in the in-process benchmark, so a handful is free and thousands on one hot function are not. The same thousands spread over many functions cost nothing extra: only the called function's observers are checked."
          >
            <Chart
              series={views.filters(run)}
              x={{ scale: 'log', title: 'Filtered observers', format: 'count' }}
              y={{ scale: 'linear', title: 'Calls per second', format: 'perSec' }}
              label="Calls per second of a function with filtered observers, by number of observers, per machine size"
            />
          </ChartCard>
        </div>
      </Section>

      {/* Memory */}
      {/* biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page. */}
      <Section
        id="memory"
        eyebrow="Memory"
        title="What it holds, and how much fits"
        container={wideContainer}
        split
        lede={
          <>
            <p>
              The engine&rsquo;s resident memory at rest as idle workers connect and observers pile up. Each VM
              size&rsquo;s memory cap is drawn across: where a line would cross it, that size runs out.
            </p>
            <SizeTable
              className="mt-6"
              columns={['Idle workers', 'Observers']}
              rows={sizes.map((p) => {
                const x = h[p.name]
                const workers = fitsIn(p, x, x?.bytes_per_worker ?? null)
                const observers = fitsIn(p, x, x?.bytes_per_observer ?? null)
                return {
                  profile: p,
                  cells: [
                    p.kind === 'host'
                      ? x?.bytes_per_worker
                        ? `${f(bytes(x.bytes_per_worker))} each`
                        : '—'
                      : workers
                        ? `~${count(Math.round(workers / 100) * 100)} fit`
                        : '—',
                    p.kind === 'host'
                      ? x?.bytes_per_observer
                        ? `${f(bytes(x.bytes_per_observer))} each`
                        : '—'
                      : observers
                        ? `~${count(Math.round(observers / 1000) * 1000)} fit`
                        : '—',
                  ],
                }
              })}
            />
            <p className="mt-3 text-[13px]">
              Fits: the size&rsquo;s cap, less the engine at rest, over the measured cost of one. Real workers register
              more functions, and real observers carry bigger filters.
            </p>
          </>
        }
      >
        <div className="mt-10 space-y-6 lg:mt-0">
          <ChartCard
            title="Engine memory, by idle workers connected"
            subtitle="Each worker announces itself and registers two functions, then only answers pings."
            read="Memory grows in a straight line with the workers: the slope is the cost of one, and it is nearly the same on every size. Where a size's cap crosses the line is how many idle workers it holds."
          >
            <Chart
              series={views.workerMemory(run)}
              x={{ scale: 'linear', title: 'Idle workers', format: 'count' }}
              y={{ scale: 'linear', title: 'Resident memory', format: 'mib' }}
              refs={caps}
              label="Engine resident memory by idle workers connected, per machine size"
            />
          </ChartCard>
          <ChartCard
            title="Engine memory, by observers held"
            subtitle="Spread over 16 functions, a small filter each, 1,000 per observing worker."
            read="Again a straight line. A line that stops short crossed its size's cap: the engine was stopped by the kernel there, and the label says so."
          >
            <Chart
              series={views.observerMemory(run)}
              x={{ scale: 'linear', title: 'Observers held', format: 'count' }}
              y={{ scale: 'linear', title: 'Resident memory', format: 'mib' }}
              refs={caps}
              label="Engine resident memory by observers held, per machine size"
            />
          </ChartCard>
        </div>
      </Section>

      {/* Overload */}
      {/* biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page. */}
      <Section
        id="overload"
        eyebrow="Overload"
        title="When the targets are slow"
        container={wideContainer}
        split
        lede={
          <>
            <p>
              Observers whose target takes 20 ms, behind calls that come far faster. Each observer runs 64 activations
              at a time and queues up to 1,024 more; past that, events are dropped and counted.
            </p>
            <p className="mt-3">
              The calls hold at 64 in flight while slow observers are added: 4, 64, 256. Every one of them keeps a full
              queue. A queued event is held once, however many observers wait on it, and all the queues together are
              capped at 64 MiB of events.
            </p>
          </>
        }
      >
        <div className="mt-10 lg:mt-0">
          <ChartCard
            title="Peak engine memory, by slow observers"
            read="The queues are bounded per observer and all together, so what still grows with the number of slow observers is their activations in flight, 64 at a time for each. The calls themselves keep flowing; the slow observers lose events, every one counted as OVERFLOW in the observer's stats."
          >
            <Chart
              series={views.overload(run)}
              x={{ scale: 'band', title: 'Slow observers on the function', format: 'count' }}
              y={{ scale: 'linear', title: 'Peak resident memory', format: 'mib' }}
              refs={caps}
              label="Peak engine memory by slow observers with full queues, per machine size"
            />
          </ChartCard>
        </div>
      </Section>

      {/* Local calls */}
      {/* biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page. */}
      <Section
        id="local"
        eyebrow="Local calls"
        title="Calls that never leave the worker"
        container={wideContainer}
        split
        lede={
          <>
            <p>
              A worker calling a function it hosts runs it in its own process. The engine hears about such a call only
              when an observer&rsquo;s filter matches it, in batched reports.
            </p>
            <p className="mt-3">
              16 tasks in one worker call its own function as fast as they can: unobserved, with an observer matching 1%
              of the calls, and with one matching them all.
            </p>
          </>
        }
      >
        <div className="mt-10 lg:mt-0">
          <ChartCard
            title="Local calls per second"
            read="Unobserved, the engine does nothing at all: no frame reaches it. Observed, it pays only for the reports and activations of the calls that match."
          >
            <LocalBars rows={views.local(run)} />
          </ChartCard>
        </div>
      </Section>

      {/* History: the headline numbers across published runs (shown from the second run on). */}
      {(bench.loadtest?.index.length ?? 0) >= 2 && bench.loadtest ? <History index={bench.loadtest.index} /> : null}

      {/* Micro-benchmarks */}
      {micro && bench.micro ? (
        // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
        <Section
          id="micro"
          eyebrow="Micro-benchmarks"
          title="Single paths, every release"
          container={wideContainer}
          lede={
            <p>
              Criterion timings of single code paths, in one process, on every release since v
              {bench.micro.releases[0]?.version}: a standard GitHub runner, 4 cores. Lower is faster; each line is a
              benchmark&rsquo;s history, and its last point is v{bench.micro.releases.at(-1)?.version}, measured{' '}
              {day(bench.micro.updated)}.
            </p>
          }
        >
          {/* Columns, not a grid: cards of different heights stack without gaps. */}
          <div className="mt-10 gap-6 lg:columns-2">
            {micro.featured.map((g) => (
              <Reveal key={g.id} className="mb-6 break-inside-avoid">
                <MicroGroupCard group={g} />
              </Reveal>
            ))}
          </div>
          {micro.more.length ? (
            <Reveal className="mt-6">
              <MicroMore groups={micro.more} />
            </Reveal>
          ) : null}
        </Section>
      ) : null}

      {/* Method */}
      {/* biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page. */}
      <Section
        id="method"
        eyebrow="Method"
        title="How we measure"
        container={wideContainer}
        split
        lede={
          <p>
            Everything on this page comes from one open-source tool that runs the same way on your machine as in CI, and
            every number links back to the run that produced it.
          </p>
        }
      >
        <div className="mt-10 space-y-10 lg:mt-0">
          <Reveal>
            <RunDiagram load={run.machine.load_vcpus} loadRam={run.machine.load_ram_mb} />
          </Reveal>
          <Reveal>
            <dl className="divide-y rounded-xl border bg-card">
              {[
                [
                  'Machine sizes',
                  `Each size is a microVM (libkrun on KVM, the virtualization iii's own sandboxes use). The engine runs pinned to its N virtual CPUs, in a cgroup capped at M with no swap: past it, the kernel stops it, and the run says where. The load generator runs in the same VM on ${run.machine.load_vcpus} vCPUs and ${f(bytes(run.machine.load_ram_mb * 1024 * 1024))} of its own and talks to the engine over the VM's loopback, so it never takes the engine's CPU and nothing proxies their sockets.`,
                ],
                [
                  'The host',
                  'The same engine as a plain process, pinned to half of the machine’s cores and not capped: the reference without a VM.',
                ],
                [
                  'The load',
                  'Real SDK workers (the Rust SDK) call, host, observe and report observed calls, as an application would. Idle workers are raw WebSockets, so ten thousand fit in one process.',
                ],
                [
                  'A step',
                  `${run.settings.step_secs ?? 10} seconds of load after a warm-up, with the engine’s CPU, memory and threads read from /proc every 250 ms. Every scenario starts on a fresh engine.`,
                ],
                [
                  'The engine',
                  'As it ships: default configuration, every span recorded, logs at info. It is the Linux glibc build, the one in the Docker image.',
                ],
                [
                  'What it is not',
                  'A shared CI machine, so runs differ by several percent: compare lines within a run, and runs by their trend. The numbers are for this machine; a faster CPU moves every line.',
                ],
              ].map(([k, v]) => (
                <div key={k} className="grid gap-1 px-5 py-4 sm:grid-cols-[150px_minmax(0,1fr)] sm:gap-6 sm:px-6">
                  <dt className="font-medium text-[14px] text-foreground">{k}</dt>
                  <dd className="text-pretty text-[14px] text-muted-foreground leading-relaxed">{v}</dd>
                </div>
              ))}
            </dl>
          </Reveal>
          <Reveal>
            <div className="rounded-xl border bg-card">
              <p className="px-5 pt-4 font-medium text-[14px] text-foreground sm:px-6">Run it yourself</p>
              <p className="px-5 pt-1 text-[13px] text-muted-foreground sm:px-6">
                Linux with KVM. The report lands in target/loadtest/.
              </p>
              <pre className="mt-3 overflow-x-auto border-t px-5 py-4 font-mono text-[12.5px] text-foreground leading-relaxed sm:px-6">
                {`cargo build --release -p iii -p iii-loadtest
target/release/iii-loadtest suite --profiles 2x1024`}
              </pre>
            </div>
          </Reveal>
          <Reveal>
            <p className={paragraph}>
              <Ext href={`${source}/tree/main/crates/iii-loadtest`}>The load test</Ext>
              {' · '}
              <Ext href={`${source}/blob/main/.github/workflows/loadtest.yml`}>The pipeline</Ext>
              {run.url ? (
                <>
                  {' · '}
                  <Ext href={run.url}>This run</Ext>
                </>
              ) : null}
              {' · '}
              <Ext href={rawData}>Raw data</Ext>
              {' · '}
              <Ext href={`${source}/tree/main/engine/benches`}>Micro-benchmarks</Ext>
              {' · '}
              <Ext href={links.docs}>Docs</Ext>
            </p>
          </Reveal>
        </div>
      </Section>

      <FloatingProfileTabs anchorId="profile-anchor" />
    </ProfileProvider>
  )
}

/** The headline numbers run after run, so a regression shows as a step down in a line. */
function History({ index }: { index: IndexEntry[] }) {
  const charts = [
    {
      title: 'Calls per second at peak',
      pick: (h: Highlights) => h.calls_per_sec,
      y: 'perSec' as const,
      axis: 'Calls per second',
    },
    {
      title: 'Activations per second, sustained',
      pick: (h: Highlights) => h.sustained_activations_per_sec,
      y: 'perSec' as const,
      axis: 'Activations per second',
    },
    {
      title: 'Engine memory per idle worker',
      pick: (h: Highlights) => (h.bytes_per_worker ? h.bytes_per_worker / 1024 / 1024 : null),
      y: 'mib' as const,
      axis: 'Resident memory',
    },
  ]
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <Section
      id="history"
      eyebrow="History"
      title="Run after run"
      container={wideContainer}
      lede={
        <p>
          The same numbers from every published run, oldest first. The machine is a shared CI runner, so a few percent
          either way is noise; a step that stays is not.
        </p>
      }
    >
      <div className="mt-10 grid gap-6 lg:grid-cols-3">
        {charts.map((c) => {
          const { series, labels } = views.history(index, c.pick)
          return (
            <ChartCard key={c.title} title={c.title}>
              <Chart
                series={series}
                x={{ scale: 'band', title: 'Run', format: 'count', labels }}
                y={{ scale: 'linear', title: c.axis, format: c.y }}
                height={240}
                label={`${c.title}, per run and machine size`}
              />
            </ChartCard>
          )
        })}
      </div>
    </Section>
  )
}

/** How one size runs: the VM, the engine on its vCPUs, the load beside it. */
function RunDiagram({ load, loadRam }: { load: number; loadRam: number }) {
  return (
    <figure className="rounded-xl border bg-card p-5 sm:p-6">
      <div className="rounded-lg border border-dashed p-4">
        <p className="font-mono text-[12px] text-muted-foreground">microVM · KVM</p>
        <div className="mt-3 grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] sm:items-center">
          <div className="rounded-lg border bg-background p-4">
            <p className="font-medium text-[14px] text-foreground">iii engine</p>
            <p className="mt-1 text-[13px] text-muted-foreground">N vCPUs, pinned</p>
            <p className="text-[13px] text-muted-foreground">cgroup cap M, no swap</p>
          </div>
          <p className="text-center font-mono text-[12px] text-muted-foreground">⇄ loopback</p>
          <div className="rounded-lg border bg-background p-4">
            <p className="font-medium text-[14px] text-foreground">Load generator</p>
            <p className="mt-1 text-[13px] text-muted-foreground">
              {load} vCPUs, {f(bytes(loadRam * 1024 * 1024))}
            </p>
            <p className="text-[13px] text-muted-foreground">SDK workers and callers</p>
          </div>
        </div>
      </div>
      <figcaption className="mt-4 text-pretty text-[13px] text-muted-foreground leading-relaxed">
        On the host, the engine&rsquo;s vCPUs and the load generator&rsquo;s run on different physical cores, so neither
        ever slows the other.
      </figcaption>
    </figure>
  )
}
