import { registerWorker } from "iii-sdk";

async function main(): Promise<void> {
  await registerWorker({ name: "my-worker" });
  console.log("worker ready");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
