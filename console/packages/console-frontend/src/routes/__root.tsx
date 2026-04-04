import type { QueryClient } from '@tanstack/react-query'
import { useQuery } from '@tanstack/react-query'
import { createRootRouteWithContext, Outlet } from '@tanstack/react-router'
import { TanStackRouterDevtools } from '@tanstack/react-router-devtools'
import { AlertTriangle } from 'lucide-react'
import { useState } from 'react'
import { Toaster } from 'sonner'
import { statusQuery } from '@/api/queries'
import { CORS_ERROR_MESSAGE, isCorsLikeFetchError } from '@/api/utils'
import { CommandPalette } from '@/components/command-palette'
import { KeyboardShortcutOverlay } from '@/components/keyboard-shortcut-overlay'
import { Sidebar } from '@/components/layout/Sidebar'
import { useKeyboard } from '@/hooks/useKeyboard'

interface RouterContext {
  queryClient: QueryClient
}

export const Route = createRootRouteWithContext<RouterContext>()({
  component: RootLayout,
})

function RootLayout() {
  const { isError: hasConnectionIssue, error: connectionError } = useQuery(statusQuery)
  const connectionMessage = isCorsLikeFetchError(connectionError)
    ? CORS_ERROR_MESSAGE
    : "Unable to connect to the iii engine. Check that it's running on the expected host and port."

  const [showCommandPalette, setShowCommandPalette] = useState(false)
  const [showHelp, setShowHelp] = useState(false)

  useKeyboard({
    onToggleCommandPalette: () => setShowCommandPalette((prev) => !prev),
    onToggleShortcutHelp: () => setShowHelp((prev) => !prev),
  })

  return (
    <div className="font-mono antialiased flex h-screen overflow-hidden bg-background text-foreground">
      <Sidebar />
      <main className="flex-1 overflow-y-auto pt-14 lg:pt-0 lg:ml-56">
        {hasConnectionIssue && (
          <div className="mx-3 md:mx-5 mt-3 bg-accent/10 border border-accent/30 rounded-[var(--radius-lg)] p-3 flex items-center gap-3">
            <AlertTriangle className="w-5 h-5 text-accent flex-shrink-0" />
            <div>
              <p className="text-sm font-sans font-medium text-accent">Connection Issue</p>
              <p className="text-xs text-secondary">{connectionMessage}</p>
            </div>
          </div>
        )}
        <Outlet />
      </main>
      <CommandPalette open={showCommandPalette} onOpenChange={setShowCommandPalette} />
      <KeyboardShortcutOverlay open={showHelp} onOpenChange={setShowHelp} />
      <Toaster
        position="bottom-right"
        toastOptions={{
          style: {
            background: 'var(--elevated)',
            border: '1px solid var(--border-subtle)',
            color: 'var(--foreground)',
            fontFamily: 'var(--font-sans)',
          },
        }}
        visibleToasts={3}
      />
      {import.meta.env.DEV && <TanStackRouterDevtools position="bottom-right" />}
    </div>
  )
}
