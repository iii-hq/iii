/* contract — the scripted-router first slice (I-E2E-001/002) + matchers. */

export const FRAME_WALK = [
  { frame: 'start', payload: 'partial = A([])', note: 'an empty assistant shell opens the stream' },
  { frame: 'text_start', payload: 'partial = A([{text: ""}])', note: 'one text block begins' },
  { frame: 'text_delta', payload: 'delta = "fixture "', note: 'partial grows to "fixture "' },
  { frame: 'text_delta', payload: 'delta = "complete"', note: 'partial grows to "fixture complete"' },
  { frame: 'text_end', payload: 'partial = A([{text: "fixture complete"}])', note: 'the block closes' },
  { frame: 'usage', payload: '{input: 8, output: 2}', note: 'scripted token accounting' },
  { frame: 'stop', payload: 'stop_reason = "end"', note: 'no error_message, no error_kind' },
  { frame: 'done', payload: 'message = A([…], {input: 8, output: 2})', note: 'the single terminal frame' },
] as const

export const MATCHER_MODES = [
  { mode: 'absent', desc: 'the field must be omitted' },
  { mode: 'present', desc: 'any value, but it must exist' },
  { mode: 'regex', desc: 'rust regex over a json string' },
  { mode: 'sha256', desc: 'digest of a string’s utf-8 bytes' },
  { mode: 'exact', desc: 'deep equality after normalizers' },
  { mode: 'subset', desc: 'every expected member at its position' },
] as const

export const C001_MATCH = [
  { field: 'writer_ref', matcher: 'subset { direction: "write" }' },
  { field: 'request_id', matcher: 'regex ^t_[0-9a-f]{32}:[0-9]+$' },
  { field: 'model · provider', matcher: 'exact "fixture-model" · "scripted"' },
  { field: 'system_prompt', matcher: 'sha256 of expected/system-prompt.txt' },
  { field: 'messages', matcher: 'exact, /0/timestamp deleted' },
  { field: 'tools', matcher: 'exact []' },
  { field: 'response_format … metadata', matcher: 'absent · five explicit absences' },
] as const

export const C002_INVARIANTS = [
  'the target receives {value: "expected"} exactly once',
  'one durable function-result message references call-1 and the correct id',
  'the second router request contains that result, in order, with the same tool entry',
  'the final assistant entry and status are terminal and durable',
  'no pending call remains and exactly two generations are consumed',
] as const

export const EXPANSION = [
  { phase: '1', items: 'denied function · repeated-send idempotency · router failure · structured output' },
  { phase: '2', items: 'steering while streaming · hook ordering · approval allow/deny · sub-agent fan-out · cancellation' },
  { phase: '3', items: 'queue redelivery/restart · dynamic registration · runtime validation' },
] as const
