import type { Handlers, StepConfig } from 'motia'
import { z } from 'zod'

const ticketSchema = z.object({
  title: z.string(),
  description: z.string(),
  priority: z.enum(['low', 'medium', 'high', 'critical']),
  customerEmail: z.string(),
})

export const config = {
  name: 'CreateTicket',
  description: 'Accepts a new support ticket via API and enqueues it for triage',
  flows: ['support-ticket-flow'],
  triggers: [
    {
      type: 'api',
      method: 'POST',
      path: '/tickets',
      bodySchema: ticketSchema,
      responseSchema: {
        200: z.object({
          ticketId: z.string(),
          status: z.string(),
          message: z.string(),
        }),
        400: z.object({ error: z.string() }),
      },
    },
  ],
  enqueues: ['ticket::created'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (request, { enqueue, logger, state }) => {
  const { title, description, priority, customerEmail } = request.body

  if (!title || !description) {
    return { status: 400, body: { error: 'Title and description are required' } }
  }

  const ticketId = `TKT-${Date.now()}-${Math.random().toString(36).substring(2, 7)}`

  const ticket = {
    id: ticketId,
    title,
    description,
    priority,
    customerEmail,
    status: 'open',
    createdAt: new Date().toISOString(),
  }

  await state.set('tickets', ticketId, ticket)

  logger.info('Ticket created', { ticketId, priority })

  await enqueue({
    topic: 'ticket::created',
    data: { ticketId, title, priority, customerEmail },
  })

  return {
    status: 200,
    body: {
      ticketId,
      status: 'open',
      message: 'Ticket created and queued for triage',
    },
  }
}
