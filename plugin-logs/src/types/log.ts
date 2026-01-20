export type Log = {
  id: string
  level: string
  time: number
  msg: string
  step: string
  traceId: string
  flows: string[]
  [key: string]: any
}
