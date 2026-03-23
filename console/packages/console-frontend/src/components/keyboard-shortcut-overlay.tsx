import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'

interface KeyboardShortcutOverlayProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

const SHORTCUTS = [
  { key: '⌘K', description: 'Open command palette' },
  { key: '⌘1-9', description: 'Navigate to page' },
  { key: '/', description: 'Focus search' },
  { key: '?', description: 'Show this help' },
  { key: 'Esc', description: 'Close panels' },
]

export function KeyboardShortcutOverlay({ open, onOpenChange }: KeyboardShortcutOverlayProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="font-sans">Keyboard Shortcuts</DialogTitle>
        </DialogHeader>
        <div className="grid grid-cols-2 gap-3 mt-4">
          {SHORTCUTS.map((shortcut) => (
            <div key={shortcut.key} className="flex items-center gap-3">
              <kbd className="inline-flex items-center justify-center min-w-[2rem] px-2 py-1 rounded-[var(--radius-sm)] bg-elevated border border-border-subtle text-xs font-mono text-foreground">
                {shortcut.key}
              </kbd>
              <span className="text-sm text-secondary font-sans">{shortcut.description}</span>
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  )
}
