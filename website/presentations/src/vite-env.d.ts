/// <reference types="vite/client" />

declare module 'virtual:spec-manifest' {
  export interface SpecEntry {
    slug: string
    title: string
    tagline: string
    /** "YYYY-MM-DD" (legacy specs may carry month-only "YYYY-MM") */
    date: string
    /** the timeline's month group, e.g. "2026 · june" */
    month: string
    /** e.g. "jun 29" — null when the date has no day component */
    dayLabel: string | null
    tags: string[]
    status: 'live' | 'draft'
    featured: boolean
    hasDeck: boolean
  }
  export const SPECS: SpecEntry[]
}
