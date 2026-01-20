import { Sidebar } from '@motiadev/ui'
import { X } from 'lucide-react'
import type { FC } from 'react'
import { EndpointPath } from '../components/endpoint-path'
import { useJsonSchemaToJson } from '../hooks/use-json-schema-to-json'
import type { ApiEndpoint } from '../types/endpoint'
import { EndpointBodyPanel } from './spec-body-panel'
import { EndpointPathParamsPanel } from './spec-path-params-panel'
import { EndpointQueryParamsPanel } from './spec-query-params-panel'
import { EndpointResponseSchema } from './spec-response-schema'

type Props = { endpoint: ApiEndpoint; onClose: () => void }

export const SpecSidePanel: FC<Props> = ({ endpoint, onClose }) => {
  const { body } = useJsonSchemaToJson(endpoint.bodySchema)

  return (
    <Sidebar
      initialWidth={600}
      subtitle={endpoint.description}
      title={<EndpointPath method={endpoint.method} path={endpoint.path} />}
      onClose={onClose}
      actions={[
        {
          icon: <X className="cursor-pointer w-4 h-4" onClick={onClose} />,
          onClick: onClose,
        },
      ]}
    >
      <EndpointPathParamsPanel endpoint={endpoint} />
      <EndpointQueryParamsPanel endpoint={endpoint} />
      <EndpointBodyPanel endpoint={endpoint} panelName="details" value={body} />
      <EndpointResponseSchema
        items={Object.entries(endpoint?.responseSchema ?? {}).map(([status, schema]) => ({
          responseCode: status,
          bodySchema: schema,
        }))}
      />
    </Sidebar>
  )
}
