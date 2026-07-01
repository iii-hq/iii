import { CodeBlock } from '@lib/components/schematic/CodeBlock'
import { useEffect, useId, useState } from 'react'

/**
 * A mermaid diagram themed to the drafting sheet (mono font, ink borders, panel
 * fills, light/dark aware). Renders the SVG client-side and re-renders when the
 * deck's data-theme flips. Falls back to the diagram source while rendering or
 * if mermaid throws.
 */

function themeVariables(dark: boolean) {
  return dark
    ? {
        background: '#111110',
        mainBkg: '#1a1916',
        primaryColor: '#1a1916',
        primaryBorderColor: '#f2f0ed',
        primaryTextColor: '#f2f0ed',
        secondaryColor: '#1f1e1c',
        tertiaryColor: '#1f1e1c',
        lineColor: '#9c9893',
        textColor: '#f2f0ed',
        nodeBorder: '#f2f0ed',
        clusterBkg: '#1f1e1c',
        clusterBorder: '#2a2926',
        titleColor: '#f2f0ed',
        edgeLabelBackground: '#111110',
        actorBkg: '#1a1916',
        actorBorder: '#f2f0ed',
        actorTextColor: '#f2f0ed',
        signalColor: '#9c9893',
        signalTextColor: '#f2f0ed',
        labelBoxBkgColor: '#1a1916',
        labelBoxBorderColor: '#2a2926',
      }
    : {
        background: '#f2f0ed',
        mainBkg: '#e9e6e2',
        primaryColor: '#e9e6e2',
        primaryBorderColor: '#0a0a0a',
        primaryTextColor: '#0a0a0a',
        secondaryColor: '#ebe8e3',
        tertiaryColor: '#f2f0ed',
        lineColor: '#6b6865',
        textColor: '#0a0a0a',
        nodeBorder: '#0a0a0a',
        clusterBkg: '#ebe8e3',
        clusterBorder: '#d8d5d0',
        titleColor: '#0a0a0a',
        edgeLabelBackground: '#f2f0ed',
        actorBkg: '#e9e6e2',
        actorBorder: '#0a0a0a',
        actorTextColor: '#0a0a0a',
        signalColor: '#6b6865',
        signalTextColor: '#0a0a0a',
        labelBoxBkgColor: '#e9e6e2',
        labelBoxBorderColor: '#d8d5d0',
      }
}

export function Mermaid({ chart }: { chart: string }) {
  const rawId = useId()
  const id = `mmd-${rawId.replace(/[^a-zA-Z0-9]/g, '')}`
  const [svg, setSvg] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false

    const render = async () => {
      // lazy-load mermaid (~1.5MB) so decks without mermaid fences never ship it
      const { default: mermaid } = await import('mermaid')
      const dark = document.documentElement.dataset.theme === 'dark'
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: 'strict',
        theme: 'base',
        fontFamily: '"Chivo Mono", ui-monospace, monospace',
        themeVariables: themeVariables(dark),
      })
      try {
        const { svg } = await mermaid.render(id, chart)
        if (!cancelled) setSvg(svg)
      } catch {
        if (!cancelled) setSvg(null)
      }
    }

    void render()
    const observer = new MutationObserver(() => void render())
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    })
    return () => {
      cancelled = true
      observer.disconnect()
    }
  }, [chart, id])

  if (!svg) {
    return <CodeBlock title="mermaid">{chart}</CodeBlock>
  }

  return (
    <div className="my-5 overflow-x-auto border border-rule bg-bg p-4">
      <div
        className="flex justify-center [&_svg]:max-w-full [&_svg]:h-auto"
        // biome-ignore lint/security/noDangerouslySetInnerHtml: trusted, locally-authored spec diagram rendered by mermaid
        dangerouslySetInnerHTML={{ __html: svg }}
      />
    </div>
  )
}
