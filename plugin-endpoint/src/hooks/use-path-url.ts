import { useMemo } from 'react'
import { useShallow } from 'zustand/react/shallow'
import {
  getPathParamsSelector,
  getQueryParamsSelector,
  type UseEndpointConfiguration,
  useEndpointConfiguration,
} from './use-endpoint-configuration'

const queryParamsSelector = (state: UseEndpointConfiguration) =>
  Object.values(getQueryParamsSelector(state)).filter(
    (param) => param.active && param.value !== '' && param.name !== '',
  )

export const usePathUrl = (path: string) => {
  const pathParams = useEndpointConfiguration(useShallow(getPathParamsSelector))
  const queryParams = useEndpointConfiguration(useShallow(queryParamsSelector))

  return useMemo(() => {
    const url = path.replace(/:(\w+)/g, (match, p1) => {
      return pathParams[p1]?.value || match
    })
    return (
      url + (queryParams.length > 0 ? `?${queryParams.map((param) => `${param.name}=${param.value}`).join('&')}` : '')
    )
  }, [path, pathParams, queryParams])
}
