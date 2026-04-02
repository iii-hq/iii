import { useNavigate } from '@tanstack/react-router'
import { useEffect } from 'react'

// Must skip shortcuts when activeElement is <input>, <textarea>, or contentEditable
const isInputFocused = () => {
  const el = document.activeElement
  if (!el) return false
  const tag = el.tagName.toLowerCase()
  if (tag === 'input' || tag === 'textarea') return true
  if ((el as HTMLElement).isContentEditable) return true
  return false
}

interface UseKeyboardOptions {
  onToggleCommandPalette: () => void
  onToggleShortcutHelp: () => void
}

export function useKeyboard({ onToggleCommandPalette, onToggleShortcutHelp }: UseKeyboardOptions) {
  const navigate = useNavigate()

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Cmd+K / Ctrl+K - command palette (always active)
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault()
        onToggleCommandPalette()
        return
      }

      if (isInputFocused()) return

      // ? - help overlay
      if (e.key === '?' && !e.metaKey && !e.ctrlKey) {
        e.preventDefault()
        onToggleShortcutHelp()
        return
      }

      // / - focus search (if a search input exists on page)
      if (e.key === '/' && !e.metaKey && !e.ctrlKey) {
        e.preventDefault()
        const searchInput = document.querySelector<HTMLInputElement>('input[placeholder*="Search"]')
        searchInput?.focus()
        return
      }

      // Cmd+1-9 page navigation
      if ((e.metaKey || e.ctrlKey) && e.key >= '1' && e.key <= '9') {
        e.preventDefault()
        const pages = [
          '/functions',
          '/triggers',
          '/states',
          '/streams',
          '/queues',
          '/dead-letter',
          '/traces',
          '/logs',
          '/config',
        ] as const
        const index = parseInt(e.key, 10) - 1
        if (index < pages.length) {
          const page = pages[index]
          // Cmd+6 (Dead Letter phantom) goes to queues DLQ tab
          if (page === '/dead-letter') {
            navigate({ to: '/queues', search: { tab: 'dead-letters' } })
          } else if (page === '/queues') {
            navigate({ to: '/queues', search: {} })
          } else {
            navigate({ to: page })
          }
        }
      }
    }

    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [navigate, onToggleCommandPalette, onToggleShortcutHelp])
}
