import { Clock, Database, Globe, ListOrdered, Zap } from 'lucide-react'

export function TriggerIcon({ type }: { type: string }) {
  const cls = 'w-3 h-3'
  switch (type) {
    case 'event':
      return <Zap className={cls} />
    case 'http':
      return <Globe className={cls} />
    case 'cron':
      return <Clock className={cls} />
    case 'queue':
      return <ListOrdered className={cls} />
    case 'state':
      return <Database className={cls} />
    default:
      return null
  }
}
