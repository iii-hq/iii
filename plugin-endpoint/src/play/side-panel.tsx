import { BackgroundEffect, Badge, Button, cn, Tabs, TabsContent, TabsList, TabsTrigger } from '@motiadev/ui'
import { Book, X } from 'lucide-react'
import { type FC, memo, useState } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { EndpointPath } from '../components/endpoint-path'
import {
  getHeadersSelector,
  getQueryParamsSelector,
  getResponseSelector,
  type UseEndpointConfiguration,
  useEndpointConfiguration,
} from '../hooks/use-endpoint-configuration'
import { usePathParams } from '../hooks/use-path-params'
import { SpecSidePanel } from '../spec/spec-side-panel'
import type { ApiEndpoint } from '../types/endpoint'
import { SidePanelBodyTab } from './side-panel-body-tab'
import { SidePanelHeadersTab } from './side-panel-headers-tab'
import { SidePanelParamsTab } from './side-panel-params-tab'
import { SidePanelResponse } from './side-panel-response'
import { TriggerButton } from './trigger-button'

type EndpointSidePanelProps = { endpoint: ApiEndpoint; onClose: () => void }

type ActiveTab = 'body' | 'headers' | 'params'

const headersCountSelector = (state: UseEndpointConfiguration) => Object.keys(getHeadersSelector(state)).length
const hasResponseSelector = (state: UseEndpointConfiguration) => getResponseSelector(state) !== undefined
const paramsCountSelector = (state: UseEndpointConfiguration) => Object.keys(getQueryParamsSelector(state)).length

export const SidePanel: FC<EndpointSidePanelProps> = memo(({ endpoint, onClose }) => {
  const isGetOrDelete = endpoint.method === 'GET' || endpoint.method === 'DELETE'
  const [activeTab, setActiveTab] = useState<ActiveTab>(isGetOrDelete ? 'params' : 'body')
  const [isSpecOpen, setIsSpecOpen] = useState(false)
  const headersCount = useEndpointConfiguration(useShallow(headersCountSelector))
  const hasResponse = useEndpointConfiguration(useShallow(hasResponseSelector))
  const pathParamsCount = usePathParams(endpoint.path).length
  const queryParamsCount = useEndpointConfiguration(useShallow(paramsCountSelector))
  const paramsCount = queryParamsCount + pathParamsCount

  return (
    <div
      className="isolate grid grid-cols-1 overflow-y-auto min-w-0 grid-rows-[auto_1fr] border-l border-border"
      data-testid="endpoint-details-panel"
    >
      <BackgroundEffect />
      <div className="grid grid-cols-[1fr_1fr_auto] items-start gap-4 px-5 py-4 border-b bg-card w-full">
        <div className="grid grid-rows-2 gap-2">
          <EndpointPath method={endpoint.method} path={endpoint.path} />
          {endpoint.description && <p className="text-sm text-muted-foreground">{endpoint.description}</p>}
        </div>
        <div className="flex items-end justify-end">
          <Button
            variant="icon"
            size="icon"
            onClick={() => setIsSpecOpen(!isSpecOpen)}
            data-testid="endpoint-spec-button"
          >
            <Book />
          </Button>
        </div>
        <Button variant="icon" size="icon" onClick={onClose}>
          <X />
        </Button>
      </div>
      <div className={cn('grid grid-cols-[minmax(380px,1fr)_minmax(auto,1fr)]', !hasResponse && 'grid-cols-1')}>
        <Tabs value={activeTab} onValueChange={(value: string) => setActiveTab(value as ActiveTab)}>
          <div className="grid grid-cols-[1fr_auto] items-center h-10 border-b px-5 bg-card">
            <TabsList>
              <TabsTrigger
                value="params"
                className="grid grid-cols-[auto_auto] gap-2 items-center cursor-pointer"
                data-testid="endpoint-params-tab"
              >
                Params
                <Badge variant="outline" className="h-4 px-1.5 text-xs">
                  {paramsCount}
                </Badge>
              </TabsTrigger>
              <TabsTrigger value="body" className="cursor-pointer" data-testid="endpoint-body-tab">
                Body
              </TabsTrigger>
              <TabsTrigger
                value="headers"
                className="grid grid-cols-[auto_auto] gap-2 items-center cursor-pointer"
                data-testid="endpoint-headers-tab"
              >
                Headers
                <Badge variant="outline" className="h-4 px-1.5 text-xs">
                  {headersCount}
                </Badge>
              </TabsTrigger>
            </TabsList>
            <TriggerButton method={endpoint.method} path={endpoint.path.toString()} />
          </div>

          <TabsContent value="params">
            <SidePanelParamsTab path={endpoint.path} />
          </TabsContent>
          <TabsContent value="body">
            <SidePanelBodyTab schema={endpoint.bodySchema} />
          </TabsContent>

          <TabsContent value="headers">
            <SidePanelHeadersTab />
          </TabsContent>
        </Tabs>
        <SidePanelResponse />
      </div>

      {isSpecOpen && <SpecSidePanel endpoint={endpoint} onClose={() => setIsSpecOpen(false)} />}
    </div>
  )
})
