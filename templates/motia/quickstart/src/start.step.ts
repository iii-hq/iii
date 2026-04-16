import type { Handlers, StepConfig } from "motia";
import { z } from "zod";

export const config = {
  // Required fields
  name: "StartTheTutorial",
  description: "Start the tutorial flow via API",
  triggers: [
    {
      type: "api",
      method: "GET",
      path: "/hello",
    },
  ],
  emits: ["hello"],

  // Some optional fields. Full list here: https://www.motia.dev/docs/api-reference
  flows: ["hello"],
  virtualEmits: ["notification.sent"], // These are visual indicators in Workbench only.
  virtualSubscribes: [], // They don't have any impact on code execution.
} satisfies StepConfig;

export const handler: Handlers<typeof config> = async (
  input,
  { emit, logger, state, trigger },
) => {
  emit({
    topic: "hello",
    data: {
      extra: `Pass any data to subscribing events with the data property. 
Use primitive types, and simple objects. Don't pass functions.
This data will be serialized and passed to handler functions in multiple languages.`,
    },
  });
};
