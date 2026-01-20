import { Panel } from '@motiadev/ui'
import type { FC } from 'react'
import { usePathParams } from '../hooks/use-path-params'
import type { ApiEndpoint } from '../types/endpoint'

type Props = {
  endpoint: ApiEndpoint
}

export const EndpointPathParamsPanel: FC<Props> = ({ endpoint }) => {
  const pathParams = usePathParams(endpoint.path)

  if (!pathParams.length) {
    return null
  }

  return (
    <Panel title="Path params" size="sm" variant="default">
      <div className="grid gap-3" style={{ gridTemplateColumns: '1fr 2fr' }}>
        {pathParams.map((param: string) => (
          <div key={param} className="font-bold leading-[36px] flex text-xs">
            {param}
          </div>
        ))}
      </div>
    </Panel>
  )
}
