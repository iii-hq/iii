import { registerWorker } from "iii-sdk";

const engineWsUrl = process.env.III_URL ?? "ws://localhost:49134";

const iii = registerWorker(engineWsUrl, {
  workerName: "my-worker",
});

iii.registerFunction(
  "hello",
  async (data: { name?: string }) => ({
    greeting: `hello, ${data?.name ?? "world"}`,
  }),
);

console.info("worker ready", { engineWsUrl });
