import { useCallback, useEffect, useRef, useState } from 'react'

const TRACE_PANEL_DEFAULT = 520
const SPAN_PANEL_DEFAULT = 400
const PANEL_MIN_WIDTH = 280
const PANEL_MAX_WIDTH = 900

const clampPanelWidth = (w: number) => Math.max(PANEL_MIN_WIDTH, Math.min(PANEL_MAX_WIDTH, w))

export interface PanelWidths {
  trace: number
  span: number
}

export interface UseResizablePanelsOptions {
  selectedSpanId: string | null
  containerRef: React.RefObject<HTMLDivElement | null>
}

export interface UseResizablePanelsReturn {
  panelWidths: PanelWidths
  isResizing: boolean
  startResize: (e: React.MouseEvent, panel: 'trace' | 'span') => void
  resetTracePanel: () => void
  resetSpanPanel: () => void
}

export function useResizablePanels({
  selectedSpanId,
  containerRef,
}: UseResizablePanelsOptions): UseResizablePanelsReturn {
  const [panelWidths, setPanelWidths] = useState<PanelWidths>({
    trace: TRACE_PANEL_DEFAULT,
    span: SPAN_PANEL_DEFAULT,
  })
  const [isResizing, setIsResizing] = useState(false)

  const panelWidthsRef = useRef({ trace: TRACE_PANEL_DEFAULT, span: SPAN_PANEL_DEFAULT })
  panelWidthsRef.current = panelWidths

  const isResizingRef = useRef<'trace' | 'span' | null>(null)
  const resizeStartRef = useRef({ x: 0, width: 0, otherWidth: 0 })
  const prevSelectedSpanRef = useRef<string | null>(null)
  const preSplitTraceWidthRef = useRef(TRACE_PANEL_DEFAULT)

  // Auto-split panels when span detail opens, restore when it closes
  useEffect(() => {
    const hadSpan = prevSelectedSpanRef.current !== null
    const hasSpan = selectedSpanId !== null

    if (hasSpan && !hadSpan) {
      preSplitTraceWidthRef.current = panelWidthsRef.current.trace
      const containerWidth = containerRef.current?.offsetWidth ?? 1200
      const halfWidth = Math.max(PANEL_MIN_WIDTH, Math.floor((containerWidth - 3) / 2))
      setPanelWidths({ trace: halfWidth, span: halfWidth })
    } else if (!hasSpan && hadSpan) {
      setPanelWidths((p) => ({ ...p, trace: preSplitTraceWidthRef.current }))
    }

    prevSelectedSpanRef.current = selectedSpanId
  }, [selectedSpanId, containerRef])

  const startResize = useCallback((e: React.MouseEvent, panel: 'trace' | 'span') => {
    e.preventDefault()
    isResizingRef.current = panel
    setIsResizing(true)
    resizeStartRef.current = {
      x: e.clientX,
      width: panel === 'trace' ? panelWidthsRef.current.trace : panelWidthsRef.current.span,
      otherWidth: panel === 'trace' ? panelWidthsRef.current.span : panelWidthsRef.current.trace,
    }
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
  }, [])

  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (!isResizingRef.current) return
      const dx = resizeStartRef.current.x - e.clientX

      if (isResizingRef.current === 'trace') {
        setPanelWidths((p) => ({ ...p, trace: clampPanelWidth(resizeStartRef.current.width + dx) }))
      } else {
        // Coupled resize: maintain total width between both panels
        const totalWidth = resizeStartRef.current.width + resizeStartRef.current.otherWidth
        const maxForPanel = totalWidth - PANEL_MIN_WIDTH
        const newSpanWidth = Math.max(
          PANEL_MIN_WIDTH,
          Math.min(maxForPanel, resizeStartRef.current.width + dx),
        )
        const newTraceWidth = totalWidth - newSpanWidth
        setPanelWidths({ trace: newTraceWidth, span: newSpanWidth })
      }
    }
    const onMouseUp = () => {
      if (isResizingRef.current) {
        isResizingRef.current = null
        setIsResizing(false)
        document.body.style.cursor = ''
        document.body.style.userSelect = ''
      }
    }
    document.addEventListener('mousemove', onMouseMove)
    document.addEventListener('mouseup', onMouseUp)
    return () => {
      document.removeEventListener('mousemove', onMouseMove)
      document.removeEventListener('mouseup', onMouseUp)
    }
  }, [])

  const resetTracePanel = useCallback(() => {
    setPanelWidths((p) => ({ ...p, trace: TRACE_PANEL_DEFAULT }))
  }, [])

  const resetSpanPanel = useCallback(() => {
    setPanelWidths((p) => ({ ...p, span: SPAN_PANEL_DEFAULT }))
  }, [])

  return {
    panelWidths,
    isResizing,
    startResize,
    resetTracePanel,
    resetSpanPanel,
  }
}
