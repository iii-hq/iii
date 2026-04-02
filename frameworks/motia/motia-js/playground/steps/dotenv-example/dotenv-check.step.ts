import { logger, type Handlers, type StepConfig } from 'motia'
import { z } from 'zod'

export const config = {
  name: 'DotenvCheck',
  description: 'Returns environment variables loaded from .env file',
  triggers: [
    {
      type: 'http',
      path: '/dotenv-check',
      method: 'GET',
      responseSchema: {
        200: z.object({
          greetingPrefix: z.string(),
          appName: z.string(),
          hasSecretKey: z.boolean(),
        }),
      },
    },
  ],
  enqueues: [],
  flows: ['dotenv-example'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async () => {
  const greetingPrefix = process.env.GREETING_PREFIX || 'NOT_SET'
  const appName = process.env.APP_NAME || 'NOT_SET'
  const hasSecretKey = !!process.env.SECRET_KEY

  logger.info('Dotenv check', { greetingPrefix, appName, hasSecretKey })

  return {
    status: 200,
    body: { greetingPrefix, appName, hasSecretKey },
  }
}
