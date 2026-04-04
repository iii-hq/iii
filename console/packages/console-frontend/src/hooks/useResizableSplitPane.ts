import { useCallback, useEffect, useRef, useState } from 'react'

const DEFAULT_RIGHT_WIDTH = 480
const MIN_WIDTH = 280
const MAX_WIDTH = 900

const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v))

export interface UseResizableSplitPaneOptions {
  rightPanelOpen: boolean
  containerRef: React.RefObject<HTMLDivElement | null>
  defaultRightWidth?: number
  minWidth?: number
  maxWidth?: number
}

export interface UseResizableSplitPaneReturn {
  rightPanelWidth: number
  isResizing: boolean
  startResize: (e: React.MouseEvent) => void
  resetPanel: () => void
}

export function useResizableSplitPane({
  rightPanelOpen,
  containerRef,
  defaultRightWidth = DEFAULT_RIGHT_WIDTH,
  minWidth = MIN_WIDTH,
  maxWidth = MAX_WIDTH,
}: UseResizableSplitPaneOptions): UseResizableSplitPaneReturn {
  const [rightPanelWidth, setRightPanelWidth] = useState(defaultRightWidth)
  const [isResizing, setIsResizing] = useState(false)

  const widthRef = useRef(defaultRightWidth)
  widthRef.current = rightPanelWidth

  const isResizingRef = useRef(false)
  const resizeStartRef = useRef({ x: 0, width: 0 })
  const prevOpenRef = useRef(false)

  // Auto-expand/collapse when rightPanelOpen changes
  useEffect(() => {
    const wasOpen = prevOpenRef.current
    const isOpen = rightPanelOpen

    if (isOpen && !wasOpen) {
      setRightPanelWidth(defaultRightWidth)
    }

    prevOpenRef.current = isOpen
  }, [rightPanelOpen, defaultRightWidth])

  // Dragging right expands the right panel (handle moves left = panel grows)
  const startResize = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    isResizingRef.current = true
    setIsResizing(true)
    resizeStartRef.current = { x: e.clientX, width: widthRef.current }
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
  }, [])

  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (!isResizingRef.current) return
      // Dragging left (negative dx) = expanding right panel
      const dx = resizeStartRef.current.x - e.clientX
      const containerWidth = containerRef.current?.offsetWidth ?? 1200
      const maxAllowed = Math.min(maxWidth, containerWidth - minWidth)
      setRightPanelWidth(clamp(resizeStartRef.current.width + dx, minWidth, maxAllowed))
    }
    const onMouseUp = () => {
      if (isResizingRef.current) {
        isResizingRef.current = false
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
  }, [containerRef, minWidth, maxWidth])

  const resetPanel = useCallback(() => {
    setRightPanelWidth(defaultRightWidth)
  }, [defaultRightWidth])

  return { rightPanelWidth, isResizing, startResize, resetPanel }
}
