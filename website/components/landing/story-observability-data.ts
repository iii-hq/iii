// Sample workload for the site's orders::create → database::execute example.
// These are function outcomes, not HTTP response-code metrics or production measurements.
const ORDER_CALLS = [
  18, 22, 16, 14, 12, 15, 19, 24, 20, 17, 26, 31, 28, 34, 40, 36, 29, 33, 42, 48, 39, 44, 51, 46, 38, 32, 41, 49, 56,
  52, 45, 60, 54, 48, 62, 68, 59, 53, 47, 64, 58, 72, 65, 57, 49, 43, 38, 34,
]

function clockLabel(halfHour: number) {
  return `${String(Math.floor(halfHour / 2)).padStart(2, '0')}:${halfHour % 2 ? '30' : '00'}`
}

export const OBSERVABILITY_BUCKETS = ORDER_CALLS.map((apiCalls, i) => {
  const apiFailures = i === 7 || i === 19 || i === 35 ? 1 : 0
  const databaseCalls = apiCalls - apiFailures
  const databaseFailures = i === 12 || i === 30 ? 1 : i >= 39 && i <= 43 ? 2 : 0
  const calls = apiCalls + databaseCalls
  const failed = apiFailures + databaseFailures
  return {
    time: `${clockLabel(i)}–${clockLabel(i + 1)} UTC`,
    apiCalls,
    databaseCalls,
    apiFailures,
    databaseFailures,
    calls,
    failed,
    completed: calls - failed,
    failureRate: (failed / calls) * 100,
  }
})

export const OBSERVABILITY_TOTALS = OBSERVABILITY_BUCKETS.reduce(
  (total, bucket) => ({ calls: total.calls + bucket.calls, failed: total.failed + bucket.failed }),
  { calls: 0, failed: 0 },
)

export function observationPath(values: number[], max: number) {
  return values
    .map((value, i) => {
      const x = ((i / (values.length - 1)) * 300).toFixed(1)
      const y = (100 - (value / max) * 100).toFixed(1)
      return i === 0 ? `M${x} ${y}` : `H${x} V${y}`
    })
    .join(' ')
}
