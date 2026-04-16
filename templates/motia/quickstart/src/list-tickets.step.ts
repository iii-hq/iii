import type { Handlers, StepConfig } from 'motia'
import { z } from 'zod'

export const config = {
  name: 'ListTickets',
  description: 'Returns all tickets from state',
  flows: ['support-ticket-flow'],
  triggers: [
    {
      type: 'api',
      method: 'GET',
      path: '/tickets',
      responseSchema: {
        200: z.object({
          tickets: z.array(z.record(z.string(), z.any())),
          count: z.number(),
        }),
      },
    },
  ],
  enqueues: [],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (_, { state, logger }) => {
  const tickets = await state.list<Record<string, unknown>>('tickets')

  logger.info('Listing tickets', { count: tickets.length })

  return {
    status: 200,
    body: { tickets, count: tickets.length },
  }
}
