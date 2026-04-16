// This is an example of a JavaScript step.
import { z } from "zod";

export const config = {
  name: "HelloFromJavaScript",
  description: "Say hello from JavaScript!",
  triggers: [
    {
      type: "event",
      topic: "hello",
      input: z.object({ extra: z.string() }),
    },
  ],
  emits: ["hello.response.javascript"],

  // Some optional fields. Full list here: https://www.motia.dev/docs/api-reference
  flows: ["hello"],
  virtualEmits: [],
  virtualSubscribes: [],
};

export const handler = async (
  input,
  { emit, logger, state, trigger }, // context object
) => {
  logger.info("Hello from JavaScript!");
  emit({ topic: "hello.response.javascript", data: { extra: "js" } });
};
