import { Panel } from '@motiadev/ui'
import type { FC } from 'react'
import ReactJson from 'react18-json-view'
import 'react18-json-view/src/dark.css'
import 'react18-json-view/src/style.css'
import type { ApiEndpoint } from '../types/endpoint'

type Props = {
  endpoint: ApiEndpoint
  value: string
  panelName: string
}

export const EndpointBodyPanel: FC<Props> = ({ endpoint, panelName, value }) => {
  const shouldHaveBody = ['post', 'put', 'patch'].includes(endpoint.method.toLowerCase())

  if (!shouldHaveBody) {
    return null
  }

  return (
    <Panel title="Body" size="sm" contentClassName="p-0" data-testid={`endpoint-body-panel__${panelName}`}>
      <ReactJson
        src={value ? JSON.parse(value) : {}}
        theme="default"
        enableClipboard={false}
        style={{ backgroundColor: 'transparent' }}
      />
    </Panel>
  )
}
