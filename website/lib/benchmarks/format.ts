/**
 * Number formatting for /benchmarks. Every figure on the page is one of four kinds: a rate (per second), a
 * duration (microseconds in the data), a size (bytes or MiB in the data) or a count. Each formatter returns the
 * number and its unit apart, so a figure can set them in two sizes.
 */

export interface Formatted {
  value: string
  unit: string
}

export const joined = ({ value, unit }: Formatted) => (unit ? `${value} ${unit}` : value)

function fixed(n: number, digits: number): string {
  // Two significant digits under 10, one under 100, none above: 4.2, 42, 420. A whole number loses its ".0".
  const places = n < 10 ? digits : n < 100 ? Math.max(0, digits - 1) : 0
  return n.toFixed(places).replace(/\.0+$/, '')
}

/** 950 → "950", 4200 → "4.2k", 72000 → "72k", 5.9M → "5.9M". */
export function compact(n: number): Formatted {
  if (n === 0) return { value: '0', unit: '' }
  if (n >= 1e6) return { value: fixed(n / 1e6, 1), unit: 'M' }
  if (n >= 1e3) return { value: fixed(n / 1e3, 1), unit: 'k' }
  return { value: n >= 10 ? Math.round(n).toString() : fixed(n, 1), unit: '' }
}

/** A rate per second: "72k/s". */
export function rate(n: number): Formatted {
  const c = compact(n)
  return { value: c.value, unit: `${c.unit}/s` }
}

/** Microseconds, in the unit that keeps the number short: 450 µs, 1.2 ms, 3.4 s. */
export function micros(us: number): Formatted {
  if (us >= 1e6) return { value: fixed(us / 1e6, 1), unit: 's' }
  if (us >= 1e3) return { value: fixed(us / 1e3, 1), unit: 'ms' }
  if (us >= 1) return { value: fixed(us, 1), unit: 'µs' }
  return { value: Math.round(us * 1000).toString(), unit: 'ns' }
}

/** Nanoseconds (criterion's unit), in the unit that keeps the number short. */
export function nanos(ns: number): Formatted {
  return ns < 1000 ? { value: Math.round(ns).toString(), unit: 'ns' } : micros(ns / 1000)
}

/** Bytes, binary units: 170 KiB, 1.6 GiB. */
export function bytes(b: number): Formatted {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB']
  let v = b
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return { value: i === 0 ? Math.round(v).toString() : fixed(v, 1), unit: units[i] }
}

export const mebibytes = (mb: number) => bytes(mb * 1024 * 1024)

/** 64000 → "64,000". */
export const count = (n: number) => Math.round(n).toLocaleString('en-US')

/** 0.6812 → "68%". */
export const percent = (ratio: number) => `${Math.round(ratio * 100)}%`

/** A profile's name for people: "2 vCPU · 1 GiB", or "Host · 8 CPUs". */
export function profileLabel(p: { kind: 'vm' | 'host'; cpus: number; mem_mb: number | null }): string {
  if (p.kind === 'host') return `Host · ${p.cpus} CPUs`
  return `${p.cpus} vCPU · ${joined(mebibytes(p.mem_mb ?? 0)).replace('.0 ', ' ')}`
}

/** The short form, for line ends and tabs: "2 vCPU", "Host". */
export function profileShort(p: { kind: 'vm' | 'host'; cpus: number }): string {
  return p.kind === 'host' ? 'Host' : `${p.cpus} vCPU`
}

/** "Oct 19, 2026" (UTC, so the server and the browser agree). */
export function day(iso: string | number): string {
  return new Date(iso).toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric', timeZone: 'UTC' })
}

/**
 * Formats for chart axes and tooltips, by name: charts are client components and the page passes them plain
 * data, so a format travels as its key.
 */
export const axisFormats = {
  /** 1, 32, 2,048, 64k. */
  count: (v: number) => (v >= 10_000 ? joined(compact(v)).replace(' ', '') : count(v)),
  /** Per-second rates on an axis titled "… per second": 72k, 1.2M. */
  perSec: (v: number) => joined(compact(v)).replace(' ', ''),
  /** Microseconds: 450 µs, 1.2 ms. */
  micros: (v: number) => joined(micros(v)),
  /** MiB: 200 MiB, 1.6 GiB. */
  mib: (v: number) => joined(mebibytes(v)),
  /** A ratio: 68%. */
  percent: (v: number) => percent(v),
} satisfies Record<string, (v: number) => string>

export type AxisFormat = keyof typeof axisFormats
