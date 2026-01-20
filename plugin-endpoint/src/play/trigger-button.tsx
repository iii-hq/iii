import { Button } from '@motiadev/ui'
import { Loader2 } from 'lucide-react'
import { memo, useState } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { getBodySelector, getHeadersSelector, useEndpointConfiguration } from '../hooks/use-endpoint-configuration'
import { usePathUrl } from '../hooks/use-path-url'

type TriggerButtonProps = {
  method: string
  path: string
}

export const TriggerButton = memo(({ method, path }: TriggerButtonProps) => {
  const { setResponse } = useEndpointConfiguration()
  const headers = useEndpointConfiguration(useShallow(getHeadersSelector))
  const body = useEndpointConfiguration(useShallow(getBodySelector))
  const pathUrl = usePathUrl(path)

  const [isLoading, setIsLoading] = useState(false)

  const onClick = async () => {
    try {
      setIsLoading(true)

      const _headers = Object.values(headers)
        .filter((header) => header.active && header.name !== '' && header.value !== '')
        .reduce(
          (acc, header) => {
            acc[header.name.toLowerCase()] = header.value
            return acc
          },
          {} as Record<string, string>,
        )

      const startTime = Date.now()
      const response = await fetch(pathUrl, {
        method: method,
        headers: _headers,
        body: ['GET', 'DELETE', 'HEAD', 'OPTIONS'].includes(method) ? null : body,
      })

      setResponse(response, startTime)
    } catch (error) {
      console.error('Error triggering endpoint:', error)
    } finally {
      setIsLoading(false)
    }
  }

  return (
    <Button variant="accent" size="sm" onClick={onClick} disabled={isLoading} data-testid="endpoint-play-button">
      {isLoading ? <Loader2 className="animate-spin" /> : 'Send'}
    </Button>
  )
})
