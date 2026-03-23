import { useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import {
  AlertTriangle,
  Database,
  GitBranch,
  Layers,
  ListOrdered,
  Server,
  Settings,
  Terminal,
  Zap,
} from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from '@/components/ui/command'

const PAGES = [
  { name: 'Functions', href: '/functions', icon: Server, shortcut: '⌘1' },
  { name: 'Triggers', href: '/triggers', icon: Zap, shortcut: '⌘2' },
  { name: 'States', href: '/states', icon: Database, shortcut: '⌘3' },
  { name: 'Streams', href: '/streams', icon: Layers, shortcut: '⌘4' },
  { name: 'Queues', href: '/queues', icon: ListOrdered, shortcut: '⌘5' },
  {
    name: 'Dead Letters',
    href: '/queues',
    icon: AlertTriangle,
    shortcut: '⌘6',
    search: { tab: 'dead-letters' as const },
  },
  { name: 'Traces', href: '/traces', icon: GitBranch, shortcut: '⌘7' },
  { name: 'Logs', href: '/logs', icon: Terminal, shortcut: '⌘8' },
  { name: 'Config', href: '/config', icon: Settings, shortcut: '⌘9' },
]

interface CommandPaletteProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function CommandPalette({ open, onOpenChange }: CommandPaletteProps) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [functions, setFunctions] = useState<Array<{ function_id: string }>>([])
  const [triggers, setTriggers] = useState<
    Array<{ id: string; trigger_type: string; function_id: string }>
  >([])
  const [streams, setStreams] = useState<Array<{ id: string }>>([])

  // Pre-fetch from React Query caches when palette opens
  useEffect(() => {
    if (!open) return
    const fnData = queryClient.getQueryData<{
      functions: Array<{ function_id: string; internal?: boolean }>
    }>(['functions'])
    if (fnData?.functions) {
      setFunctions(fnData.functions.filter((f) => !f.internal))
    }
    const trigData = queryClient.getQueryData<{
      triggers: Array<{ id: string; trigger_type: string; function_id: string; internal?: boolean }>
    }>(['triggers'])
    if (trigData?.triggers) {
      setTriggers(trigData.triggers.filter((t) => !t.internal))
    }
    const streamData = queryClient.getQueryData<{
      streams: Array<{ id: string; internal?: boolean }>
    }>(['streams'])
    if (streamData?.streams) {
      setStreams(streamData.streams.filter((s) => !s.internal))
    }
  }, [open, queryClient])

  const runCommand = useCallback(
    (command: () => void) => {
      onOpenChange(false)
      command()
    },
    [onOpenChange],
  )

  return (
    <CommandDialog open={open} onOpenChange={onOpenChange}>
      <CommandInput placeholder="Type a command or search..." />
      <CommandList>
        <CommandEmpty>No results found.</CommandEmpty>
        <CommandGroup heading="Pages">
          {PAGES.map((page) => {
            const Icon = page.icon
            return (
              <CommandItem
                key={`${page.href}-${page.name}`}
                onSelect={() =>
                  runCommand(() => {
                    if ('search' in page) {
                      navigate({ to: '/queues', search: page.search })
                    } else if (page.href === '/queues') {
                      navigate({ to: '/queues', search: {} })
                    } else {
                      navigate({ to: page.href })
                    }
                  })
                }
              >
                <Icon className="mr-2 h-4 w-4" />
                <span>{page.name}</span>
                <span className="ml-auto text-xs text-secondary font-mono">{page.shortcut}</span>
              </CommandItem>
            )
          })}
        </CommandGroup>
        {functions.length > 0 && (
          <>
            <CommandSeparator />
            <CommandGroup heading="Functions">
              {functions.slice(0, 10).map((fn) => (
                <CommandItem
                  key={fn.function_id}
                  onSelect={() =>
                    runCommand(() => navigate({ to: '/functions', search: { q: fn.function_id } }))
                  }
                >
                  <Server className="mr-2 h-4 w-4" />
                  <span className="font-mono text-xs">{fn.function_id}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </>
        )}
        {triggers.length > 0 && (
          <>
            <CommandSeparator />
            <CommandGroup heading="Triggers">
              {triggers.slice(0, 10).map((t) => (
                <CommandItem
                  key={t.id}
                  onSelect={() => runCommand(() => navigate({ to: '/triggers' }))}
                >
                  <Zap className="mr-2 h-4 w-4" />
                  <span className="font-mono text-xs">
                    {t.trigger_type}: {t.function_id}
                  </span>
                </CommandItem>
              ))}
            </CommandGroup>
          </>
        )}
        {streams.length > 0 && (
          <>
            <CommandSeparator />
            <CommandGroup heading="Streams">
              {streams.slice(0, 10).map((s) => (
                <CommandItem
                  key={s.id}
                  onSelect={() => runCommand(() => navigate({ to: '/streams' }))}
                >
                  <Layers className="mr-2 h-4 w-4" />
                  <span className="font-mono text-xs">{s.id}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </>
        )}
      </CommandList>
    </CommandDialog>
  )
}
