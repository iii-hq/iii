import { queue, api, step } from 'motia'
import { z } from 'zod'

/**
 * Another multi-trigger step: handles escalation from either an SLA breach
 * (queue trigger) or a manual escalation request (API trigger).
 *
 * Uses ctx.getData() to extract the shared ticketId field regardless of
 * which trigger fired.
 */

const breachSchema = z.object({
  ticketId: z.string(),
  priority: z.string(),
  title: z.string(),
  ageMinutes: z.number(),
})

const manualEscalateSchema = z.object({
  ticketId: z.string(),
  reason: z.string(),
})

export const stepConfig = {
  name: 'EscalateTicket',
  description: 'Multi-trigger: escalates tickets from SLA breach or manual request',
  flows: ['support-ticket-flow'],
  triggers: [
    queue('ticket::sla-breached', { input: breachSchema }),
    api('POST', '/tickets/escalate', {
      bodySchema: manualEscalateSchema,
      responseSchema: {
        200: z.object({ ticketId: z.string(), escalatedTo: z.string(), message: z.string() }),
      },
    }),
  ],
  enqueues: [],
}

export const { config, handler } = step(stepConfig, async (input, ctx) => {
  const data = ctx.getData()
  const ticketId = (data as { ticketId: string }).ticketId

  ctx.logger.info('Escalating ticket', { ticketId, triggerType: ctx.trigger.type })

  return ctx.match({
    queue: async (breach) => {
      ctx.logger.warn('Auto-escalation from SLA breach', {
        ticketId: breach.ticketId,
        ageMinutes: breach.ageMinutes,
        priority: breach.priority,
      })

      await ctx.state.set('tickets', `${breach.ticketId}:escalation`, {
        escalatedTo: 'engineering-lead',
        reason: `SLA breach: ${breach.ageMinutes} minutes without resolution`,
        method: 'auto',
        escalatedAt: new Date().toISOString(),
      })
    },

    api: async (request) => {
      const { ticketId, reason } = request.body

      ctx.logger.info('Manual escalation via API', { ticketId, reason })

      await ctx.state.set('tickets', `${ticketId}:escalation`, {
        escalatedTo: 'engineering-lead',
        reason,
        method: 'manual',
        escalatedAt: new Date().toISOString(),
      })

      return {
        status: 200,
        body: {
          ticketId,
          escalatedTo: 'engineering-lead',
          message: 'Ticket escalated successfully',
        },
      }
    },
  })
})
