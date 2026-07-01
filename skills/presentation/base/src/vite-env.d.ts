/// <reference types="vite/client" />

declare module 'virtual:spec-manifest' {
  export interface SpecEntry {
    slug: string
    title: string
    tagline: string
    date: string
    month: string
    tags: string[]
    status: 'live' | 'draft'
    featured: boolean
    hasDeck: boolean
  }
  export const SPECS: SpecEntry[]
}
