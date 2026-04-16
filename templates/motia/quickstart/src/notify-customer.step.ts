import type { Handlers, StepConfig } from 'motia'
import { z } from 'zod'

const triagedSchema = z.object({
  ticketId: z.string(),
  assignee: z.string(),
  priority: z.string(),
  title: z.string(),
})

export const config = {
  name: 'NotifyCustomer',
  description: 'Sends a notification when a ticket has been triaged',
  flows: ['support-ticket-flow'],
  triggers: [
    {
      type: 'queue',
      topic: 'ticket::triaged',
      input: triagedSchema,
    },
  ],
  enqueues: [],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input, { logger, state }) => {
  const { ticketId, assignee, priority, title } = input

  logger.info('Sending customer notification', { ticketId, assignee })

  const ticket = await state.get<{ customerEmail: string }>('tickets', ticketId)
  const redactedEmail = ticket?.customerEmail?.replace(/(?<=.{2}).(?=.*@)/g, '*') ?? 'unknown'

  logger.info('Notification sent', {
    ticketId,
    assignee,
    priority,
    title,
    email: redactedEmail,
  })
}
