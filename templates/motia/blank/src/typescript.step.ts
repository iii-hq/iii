// This is an example of a TypeScript step.
// Learn more about producing steps in TypeScript, JavaScript,
// and Python at: https://www.motia.dev/docs/concepts/steps
import type { EventConfig, Handlers } from "@iii-dev/motia";
import * as z from "zod";

export const config: EventConfig = {
  type: "event",
  name: "HelloFromTypeScript",
  subscribes: ["hello"],
  input: z.object({ extra: z.string() }),
  emits: ["hello.response.typescript"],
  flows: ["hello"],
  description: "Say hello from TypeScript!",
  virtualEmits: [],
  virtualSubscribes: [],
};

export const handler: Handlers["HelloFromTypeScript"] = async (
  payload,
  { emit, logger, state }, // context object
) => {
  logger.info("Hello from TypeScript!");
  emit({ topic: "hello.response.typescript", data: { extra: "ts" } });
};
