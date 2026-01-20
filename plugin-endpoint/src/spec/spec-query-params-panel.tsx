import { Panel } from '@motiadev/ui'
import { type FC, Fragment } from 'react'
import type { ApiEndpoint } from '../types/endpoint'

type Props = {
  endpoint: ApiEndpoint
}

export const EndpointQueryParamsPanel: FC<Props> = ({ endpoint }) => {
  if (!endpoint.queryParams?.length) {
    return null
  }

  return (
    <Panel title="Query params" size="sm" variant="outlined">
      <div className="grid gap-3" style={{ gridTemplateColumns: '1fr 2fr', gridTemplateRows: '1fr 1fr' }}>
        {endpoint.queryParams.map((param: { name: string; description: string }) => (
          <Fragment key={param.name}>
            <div className="font-bold leading-[36px] flex text-xs">{param.name}</div>
            <div className="flex items-center text-xs ">
              <span>{param.description}</span>
            </div>
          </Fragment>
        ))}
      </div>
    </Panel>
  )
}
