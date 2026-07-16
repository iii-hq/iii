// Hand-written declarations for manifest.mjs (JSDoc-typed JS), so the Astro
// pages/endpoints and scripts that import it type-check.

export interface SpecDoc {
  file: string
  label: string
}

export interface Spec {
  slug: string
  title: string
  tagline: string
  /** "YYYY-MM-DD" (legacy specs may be month-only "YYYY-MM") */
  date: string
  /** e.g. "2026 · june" */
  month: string
  /** e.g. "jun 29"; null when the date has no day component */
  dayLabel: string | null
  tags: string[]
  status: 'live' | 'draft'
  featured: boolean
  hasDeck: boolean
}

export const ROOT: string
export const SPECS_DIR: string

export function parseFrontmatter(raw: string): {
  fields: Record<string, string | string[] | boolean>
  unknown: string[]
  present: boolean
}
export function stripFrontmatter(md: string): string
export function firstHeading(md: string): string | null
export function monthLabel(date?: string): string
export function dayLabel(date?: string): string | null
export function listSpecDocs(slug: string): SpecDoc[]
export function readSpecs(): { specs: Spec[]; warnings: string[] }
export function specManifestPlugin(): {
  name: string
  resolveId(id: string): string | undefined
  load(id: string): string | undefined
}
