import type { Handlers, StepConfig } from "motia";

const SLA_THRESHOLDS_MS: Record<string, number> = {
  critical: 15 * 60 * 1000, // 15 minutes
  high: 60 * 60 * 1000, // 1 hour
  medium: 4 * 60 * 60 * 1000, // 4 hours
  low: 24 * 60 * 60 * 1000, // 24 hours
};

export const config = {
  name: "SlaMonitor",
  description: "Cron job that checks for SLA breaches on open tickets",
  flows: ["support-ticket-flow"],
  triggers: [
    {
      type: "cron",
      expression: "0/30 * * * * *", // every 30 seconds, iii has granularity down to the seconds position
    },
  ],
  enqueues: ["ticket::sla-breached"],
} as const satisfies StepConfig;

export const handler: Handlers<typeof config> = async (
  _,
  { state, logger, enqueue },
) => {
  logger.info("Running SLA compliance check");

  const tickets = await state.list<{
    id: string;
    priority: string;
    status: string;
    createdAt: string;
    title: string;
  }>("tickets");

  const now = Date.now();
  let breaches = 0;

  for (const ticket of tickets) {
    if (ticket.status !== "open" || !ticket.createdAt) continue;

    const age = now - new Date(ticket.createdAt).getTime();
    const threshold =
      SLA_THRESHOLDS_MS[ticket.priority] ?? SLA_THRESHOLDS_MS.medium;

    if (age > threshold) {
      breaches++;
      logger.warn("SLA breach detected", {
        ticketId: ticket.id,
        priority: ticket.priority,
        ageMinutes: Math.round(age / 60_000),
      });

      await enqueue({
        topic: "ticket::sla-breached",
        data: {
          ticketId: ticket.id,
          priority: ticket.priority,
          title: ticket.title,
          ageMinutes: Math.round(age / 60_000),
        },
      });
    }
  }

  logger.info("SLA check complete", { totalTickets: tickets.length, breaches });
};
