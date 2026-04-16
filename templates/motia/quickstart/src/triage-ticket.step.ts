import { api, queue, cron, step } from "motia";
import { z } from "zod";

/**
 * Multi-trigger step: demonstrates a single step responding to three trigger types.
 *
 * - queue:  automatically triages newly created tickets
 * - api:    lets a support agent manually re-triage any ticket
 * - cron:   periodically sweeps for untriaged tickets
 *
 * The handler uses ctx.match() to route to the correct logic per trigger,
 * and ctx.getData() to access shared fields when the input shape overlaps.
 */

const ticketEventSchema = z.object({
  ticketId: z.string(),
  title: z.string(),
  priority: z.string(),
  customerEmail: z.string(),
});

const manualTriageSchema = z.object({
  ticketId: z.string(),
  assignee: z.string(),
  priority: z.enum(["low", "medium", "high", "critical"]),
});

export const stepConfig = {
  name: "TriageTicket",
  description:
    "Multi-trigger: auto-triage from queue, manual triage via API, sweep via cron",
  flows: ["support-ticket-flow"],
  triggers: [
    queue("ticket::created", { input: ticketEventSchema }),
    api("POST", "/tickets/triage", {
      bodySchema: manualTriageSchema,
      responseSchema: {
        200: z.object({
          ticketId: z.string(),
          assignee: z.string(),
          status: z.string(),
        }),
        404: z.object({ error: z.string() }),
      },
    }),
    cron("0 */5 * * * * *"), // every 5 minutes
  ],
  enqueues: ["ticket::triaged"],
};

export const { config, handler } = step(stepConfig, async (input, ctx) => {
  return ctx.match({
    queue: async (queueInput) => {
      const { ticketId, title, priority } = queueInput;

      ctx.logger.info("Auto-triaging ticket from queue", {
        ticketId,
        priority,
      });

      const assignee =
        priority === "critical" || priority === "high"
          ? "senior-support"
          : "support-pool";

      await ctx.state.set("tickets", `${ticketId}:triage`, {
        ticketId,
        assignee,
        triagedAt: new Date().toISOString(),
        method: "auto",
      });

      await ctx.enqueue({
        topic: "ticket::triaged",
        data: { ticketId, assignee, priority, title },
      });

      ctx.logger.info("Ticket auto-triaged", { ticketId, assignee });
    },

    api: async (request) => {
      const { ticketId, assignee, priority } = request.body;

      const existing = await ctx.state.get("tickets", ticketId);
      if (!existing) {
        return { status: 404, body: { error: `Ticket ${ticketId} not found` } };
      }

      ctx.logger.info("Manual triage via API", { ticketId, assignee });

      await ctx.state.set("tickets", `${ticketId}:triage`, {
        ticketId,
        assignee,
        priority,
        triagedAt: new Date().toISOString(),
        method: "manual",
      });

      await ctx.enqueue({
        topic: "ticket::triaged",
        data: { ticketId, assignee, priority, title: "manual-triage" },
      });

      return {
        status: 200,
        body: { ticketId, assignee, status: "triaged" },
      };
    },

    cron: async () => {
      ctx.logger.info("Running untriaged ticket sweep");

      const allTickets = await ctx.state.list<{
        id: string;
        status: string;
        priority: string;
        title: string;
      }>("tickets");
      let swept = 0;

      for (const ticket of allTickets) {
        const triage = await ctx.state.get("tickets", `${ticket.id}:triage`);
        if (!triage && ticket.status === "open") {
          ctx.logger.warn("Found untriaged ticket during sweep", {
            ticketId: ticket.id,
          });

          await ctx.enqueue({
            topic: "ticket::triaged",
            data: {
              ticketId: ticket.id,
              assignee: "support-pool",
              priority: ticket.priority || "medium",
              title: ticket.title || "unknown",
            },
          });
          swept++;
        }
      }

      ctx.logger.info("Sweep complete", { sweptCount: swept });
    },
  });
});
