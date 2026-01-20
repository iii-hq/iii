import { Button } from '@motiadev/ui'
import { Plus } from 'lucide-react'
import { useCallback, useMemo } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { ConfigurationListItem } from '../components/configuration-list-item'
import { EndpointPathPreview } from '../components/endpoint-path-preview'
import {
  getPathParamsSelector,
  getQueryParamsSelector,
  useEndpointConfiguration,
} from '../hooks/use-endpoint-configuration'

type SidePanelParamsTabProps = {
  path: string
}

export const SidePanelParamsTab = ({ path }: SidePanelParamsTabProps) => {
  const { setQueryParams, removeQueryParams, setPathParams } = useEndpointConfiguration()
  const queryParams = useEndpointConfiguration(useShallow(getQueryParamsSelector))
  const pathParams = useEndpointConfiguration(useShallow(getPathParamsSelector))
  const pathParamsConfig = useMemo(() => {
    const params = path.match(/:(\w+)/g)
    return (
      params?.map((param) => {
        return { name: param.slice(1), value: pathParams[param.slice(1)]?.value ?? '', active: true }
      }) ?? []
    )
  }, [path])

  const addParam = useCallback(() => {
    const newParam = {
      name: '',
      value: '',
      active: true,
    }
    setQueryParams({ ...queryParams, [new Date().getTime().toString()]: newParam })
  }, [queryParams, setQueryParams])

  const updateParam = useCallback(
    (key: string, field: 'name' | 'value' | 'active', value: string | boolean) => {
      if (!key) return
      setQueryParams({ ...queryParams, [key]: { ...queryParams[key], [field]: value } })
    },
    [queryParams, setQueryParams],
  )

  const updatePathParam = useCallback(
    (key: string, field: 'name' | 'value' | 'active', value: string | boolean) => {
      if (!key) return
      setPathParams({ ...pathParams, [key]: { ...pathParams[key], [field]: value } })
    },
    [pathParams, setPathParams],
  )

  return (
    <div className="h-full grid grid-rows-[auto_auto_1fr]">
      <div className="grid px-4 border-b h-10 items-center grid-cols-[auto_1fr]">
        <Button size="sm" onClick={addParam}>
          <Plus className="h-3 w-3" />
          Add
        </Button>
      </div>
      <EndpointPathPreview path={path} />

      <div className="grid grid-rows-[1fr_1fr]">
        {pathParamsConfig.length > 0 && (
          <div className="p-2">
            <div className="text-sm font-medium pl-3">Path variables</div>
            {pathParamsConfig.map((pathName) => (
              <ConfigurationListItem
                key={pathName.name}
                value={{ name: pathName.name, value: pathName.value, active: pathName.active }}
                id={pathName.name}
                required={true}
                onUpdate={updatePathParam}
              />
            ))}
          </div>
        )}

        <div className="p-2 border-b border-border">
          <div className="text-sm font-medium pl-3">Query parameters</div>
          {Object.entries(queryParams).map(([key, param]) => (
            <ConfigurationListItem
              key={key}
              value={param}
              id={key}
              onUpdate={updateParam}
              onRemove={removeQueryParams}
            />
          ))}

          {Object.entries(queryParams).length === 0 && (
            <div className="grid grid-cols-1 items-center h-full">
              <div className="text-sm text-muted-foreground text-center">
                There are no query params in this endpoint
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
