export const formatDuration = (duration?: number) => {
  if (duration === undefined || duration === null) return 'N/A'
  if (duration < 1000) return `${duration}ms`
  if (duration < 60000) return `${(duration / 1000).toFixed(1)}s`
  if (duration < 3600000) return `${(duration / 60000).toFixed(1)}min`
  return `${(duration / 3600000).toFixed(1)}h`
}
