import { z } from 'zod'
import { iii } from './iii'

const inputSchema = z.object({
  scope: z.string(),
  key: z.string(),
})

const outputSchema = z.object({
  value: z.string(),
})

async function helloWorld(input: z.infer<typeof inputSchema>): Promise<z.infer<typeof outputSchema>> {
  return { value: `${input.scope}::${input.key}` }
}

iii.registerFunction('example::hello-world', helloWorld, {
  description: 'description',
  request_format: z.toJSONSchema(inputSchema),
  response_format: z.toJSONSchema(outputSchema),
})
