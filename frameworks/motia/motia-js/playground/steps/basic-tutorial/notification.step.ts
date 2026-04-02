import { type Handlers, jsonSchema, logger, type StepConfig } from 'motia'
import { z } from 'zod'

export const config = {
  name: 'Notification',
  description: 'Sends notifications to users',
  flows: ['basic-tutorial'],
  triggers: [
    {
      type: 'queue',
      topic: 'notification',
      input: jsonSchema(
        z.object({
          templateId: z.string(),
          email: z.string(),
          templateData: z.record(z.string(), z.any()),
        }),
      ),
    },
  ],
  enqueues: [],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input, { traceId }) => {
  const { email, ...data } = input || {}
  const redactedEmail = email?.replace(/(?<=.{2}).(?=.*@)/g, '*')

  logger.info('Processing Notification', { data, email: redactedEmail })

  logger.info('New notification sent', {
    templateId: data.templateId,
    email: redactedEmail,
    templateData: data.templateData,
    traceId,
  })
}
