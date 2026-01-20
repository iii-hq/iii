import { useStreamItem } from '@motiadev/stream-client-react'

export const useStateStream = (object: Record<string, any> | undefined) => {
  const { __motia, ...rest } = typeof object === 'string' ? {} : object || {}
  const { data } = useStreamItem(__motia)
  const originalData = Array.isArray(object) ? object : rest || object

  if (typeof object === 'string') {
    return {
      data: object,
      originalData: object,
      isStreamed: false,
    }
  }

  return {
    data: JSON.stringify(data || originalData, null, 2),
    originalData,
    isStreamed: !!__motia,
  }
}
