// This is an example of a TypeScript step.
// Every step is composed of a config, and a handler function.
import type { Handlers, StepConfig } from "@iii-dev/motia";
import { z } from "zod";

// Every Step has a config that describes how a step is triggered,
// what events it emits, and what "flows" it's part of

// There are a few different types of triggers:

// api = Create routes (ex. /hello) that trigger your step
// event = Have specific events trigger your step
// cron = Have specific times trigger your step
export const config = {
  // Each config has a name, a triggers array defining what triggers it,
  // and what events it might emit.
  name: "HelloFromTypeScript",
  description: "Say hello from TypeScript!",
  // The triggers array can contain multiple triggers of different types.
  // When another Step emits a "hello" event then the handler below will run.
  // This step also runs every 30 minutes via cron, and has an API endpoint
  // that could be called (though we don't use it in this example).
  triggers: [
    {
      type: "event",
      topic: "hello",
      input: z.object({ extra: z.string() }),
    },
    {
      type: "api",
      method: "GET",
      path: "/hello/ts",
    },
    {
      type: "cron",
      expression: "*/30 * * * *",
    },
  ],
  emits: ["hello.response.typescript"],

  // These fields are optional but the flow field is very useful
  // for visual organization inside this Workbench.
  // If you have a series of Steps that all complete one big task or workflow
  // then adding them to the same flow makes visualizing them in Workbench easier.
  flows: ["hello"],
  virtualEmits: [],
  virtualSubscribes: [],
} satisfies StepConfig;

// This is a handler, it's the code that will run when the conditions
// defined in the config are met. Every handler gets an input and
// a context that contains useful functions to emit events, create logs,
// modify state, etc. Checkout the docs to see all that the context can do!

// Now click the Run button to run this code. Then look at the Tracing tab
// at the bottom of the screen to see what happened.
// Motia makes it easy to follow the flow of your program.
export const handler: Handlers<typeof config> = async (
  input,
  { emit, logger, state, trigger }, // context object
) => {
  logger.info("Hello from TypeScript!");
  emit({ topic: "hello.response.typescript", data: { extra: "ts" } });
};
