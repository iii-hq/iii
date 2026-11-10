// iii.dev email signup worker. iii.dev/api/subscribe is proxied here by
// CloudFront (infra/terraform/website/cloudfront.tf). RESEND_API_KEY comes from
// the iii Cloud environment's secrets; the rest is set in worker-compose.yaml.

import { registerWorker } from 'iii-sdk'

import { createSubscribe, type Request } from './subscribe'

function required(name: string): string {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is not set`)
  return value
}

/**
 * `RESEND_SEGMENTS`: `source=segment-id` pairs, comma separated
 * (`website=bf68…,ade=…`). A source with an empty ID is accepted but joins no
 * segment.
 */
function parseSegments(raw: string): Record<string, string> {
  const segments: Record<string, string> = {}
  for (const pair of raw
    .split(',')
    .map((p) => p.trim())
    .filter(Boolean)) {
    const [source, id = ''] = pair.split('=').map((p) => p.trim())
    if (!source) throw new Error(`RESEND_SEGMENTS: "${pair}" has no source`)
    segments[source] = id
  }
  if (!Object.hasOwn(segments, 'website')) segments.website = ''
  return segments
}

const subscribe = createSubscribe({
  apiKey: required('RESEND_API_KEY'),
  templateId: required('RESEND_TEMPLATE_ID'),
  from: required('RESEND_FROM'),
  subject: required('RESEND_SUBJECT'),
  segments: parseSegments(process.env.RESEND_SEGMENTS ?? ''),
  defaultSource: 'website',
  allowedOrigins: (process.env.ALLOWED_ORIGINS ?? '').split(',').filter(Boolean),
  maxWelcomesPerMinute: process.env.MAX_WELCOMES_PER_MINUTE ? Number(process.env.MAX_WELCOMES_PER_MINUTE) : undefined,
})

const iii = registerWorker(process.env.III_URL ?? 'ws://localhost:49134')

iii.registerFunction('subscribe::create', (req: Request) => subscribe(req), {
  description: 'Add an email to Resend Contacts and send the iii welcome email once',
})
// iii 0.26: an HTTP route is an observer of the http worker's http::on-request
// (registerTrigger is gone, and a 0.26 engine drops it without an error).
const route = iii.observe(
  { function_id: 'http::on-request', args: { method: 'POST', path: '/api/subscribe' } },
  'subscribe::create',
)
await route.ready()

process.on('SIGTERM', async () => {
  await iii.shutdown()
  process.exit(0)
})
