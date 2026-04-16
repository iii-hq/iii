// This is an example step that subscribes to multiple events
// and will run any time it hears any of them.
import type { Handlers, StepConfig } from "motia";
import { z } from "zod";

export const config = {
  name: "ListensToMultipleEvents",
  description: "Listens to several events, and runs for each",
  triggers: [
    {
      type: "event",
      topic: "hello.response.typescript",
      input: z.object({ extra: z.string() }),
    },
    {
      type: "event",
      topic: "hello.response.javascript",
      input: z.object({ extra: z.string() }),
    },
    {
      type: "event",
      topic: "hello.response.python",
      input: z.object({ extra: z.string() }),
    },
  ],
  emits: [],
  flows: ["hello"],
  virtualEmits: [],
  virtualSubscribes: [],
} satisfies StepConfig;

export const handler: Handlers<typeof config> = async (
  input,
  { emit, logger, state, trigger }, // context object
) => {
  logger.info(`I heard an event, it had the payload: ${JSON.stringify(input)}`);
};
