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

const subscribe = createSubscribe({
  apiKey: required('RESEND_API_KEY'),
  templateId: required('RESEND_TEMPLATE_ID'),
  from: required('RESEND_FROM'),
  subject: required('RESEND_SUBJECT'),
  segmentId: process.env.RESEND_SEGMENT_ID || undefined,
  allowedOrigins: (process.env.ALLOWED_ORIGINS ?? '').split(',').filter(Boolean),
})

const iii = registerWorker(process.env.III_URL ?? 'ws://localhost:49134')

iii.registerFunction('subscribe::create', (req: Request) => subscribe(req), {
  description: 'Add an email to Resend Contacts and send the iii welcome email once',
})
iii.registerTrigger({
  type: 'http',
  function_id: 'subscribe::create',
  config: { api_path: '/api/subscribe', http_method: 'POST' },
})

process.on('SIGTERM', async () => {
  await iii.shutdown()
  process.exit(0)
})
