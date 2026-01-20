import { useStreamGroup } from '@motiadev/stream-client-react'
import { useMemo } from 'react'
import type { ApiEndpoint } from '../types/endpoint'

export const useGetEndpoints = () => {
  const { data: endpoints } = useStreamGroup<ApiEndpoint>({
    streamName: '__motia.api-endpoints',
    groupId: 'default',
  })

  const groupedEndpoints = useMemo(() => {
    return endpoints.reduce(
      (acc, endpoint) => {
        if (endpoint.flows && endpoint.flows.length > 0) {
          endpoint.flows.forEach((flow) => {
            acc[flow] = acc[flow] || []
            acc[flow].push(endpoint)
          })
        } else {
          acc['no-flow'] = acc['no-flow'] || []
          acc['no-flow'].push(endpoint)
        }
        return acc
      },
      {} as Record<string, ApiEndpoint[]>,
    )
  }, [endpoints])

  return { endpoints, groupedEndpoints }
}
