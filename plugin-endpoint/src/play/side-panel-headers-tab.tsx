import { Button } from '@motiadev/ui'
import { Plus } from 'lucide-react'
import { useCallback } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { ConfigurationListItem } from '../components/configuration-list-item'
import { getHeadersSelector, useEndpointConfiguration } from '../hooks/use-endpoint-configuration'

export const SidePanelHeadersTab = () => {
  const { setHeaders, removeHeaders } = useEndpointConfiguration()
  const headers = useEndpointConfiguration(useShallow(getHeadersSelector))

  const addHeader = useCallback(() => {
    const newHeader = {
      name: '',
      value: '',
      active: true,
    }
    setHeaders({ ...headers, [new Date().getTime().toString()]: newHeader })
  }, [headers, setHeaders])

  const updateHeader = useCallback(
    (key: string, field: 'name' | 'value' | 'active', value: string | boolean) => {
      if (!key) return
      setHeaders({ ...headers, [key]: { ...headers[key], [field]: value } })
    },
    [headers, setHeaders],
  )

  return (
    <div className="h-full max-h-full grid grid-rows-[auto_1fr]">
      <div className="grid px-4 border-b h-10 items-center grid-cols-[auto_1fr]">
        <Button size="sm" onClick={addHeader}>
          <Plus className="h-3 w-3" />
          Add
        </Button>
      </div>

      <div className="p-2">
        {Object.entries(headers).map(([key, header]) => (
          <ConfigurationListItem key={key} value={header} id={key} onUpdate={updateHeader} onRemove={removeHeaders} />
        ))}

        {Object.entries(headers).length === 0 && (
          <div className="grid grid-cols-1 items-center h-full">
            <div className="text-sm text-muted-foreground text-center">There are no headers in this endpoint</div>
          </div>
        )}
      </div>
    </div>
  )
}
