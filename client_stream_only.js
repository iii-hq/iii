import path from "node:path";
import process from "node:process";
import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";

const PROTO_PATH = path.join(process.cwd(), "proto", "engine.proto");
const packageDefinition = await protoLoader.load(PROTO_PATH, {
  keepCase: false,
  longs: String,
  enums: String,
  defaults: true,
  oneofs: true,
});

const enginePkg = grpc.loadPackageDefinition(packageDefinition).engine.v1;

const target = process.env.ENGINE_ADDR ?? "localhost:50051";
const message = process.argv[2] ?? "streaming payload from node";
const chunkSizeArg = process.argv[3] ?? process.env.CHUNK_SIZE;
const delayMsArg = process.argv[4] ?? process.env.DELAY_MS;
const prefixArg = process.argv[5] ?? process.env.PREFIX;
const suffixArg = process.argv[6] ?? process.env.SUFFIX;

const chunkSize = chunkSizeArg ? String(chunkSizeArg) : undefined;
const delayMs = delayMsArg ? String(delayMsArg) : undefined;
const prefix = prefixArg ? String(prefixArg) : undefined;
const suffix = suffixArg ? String(suffixArg) : undefined;

const request = {
  service: "text-formatter",
  method: "stream_format",
  payload: message,
  meta: {
    ...(chunkSize ? { chunk_size: chunkSize } : {}),
    ...(delayMs ? { delay_ms: delayMs } : {}),
    ...(prefix ? { prefix } : {}),
    ...(suffix ? { suffix } : {}),
  },
};

const client = new enginePkg.Engine(target, grpc.credentials.createInsecure());

console.log(`Sending streaming request to ${target}:`);
console.log(`  payload="${message}"`);
if (chunkSize) {
  console.log(`  chunk_size=${chunkSize}`);
}
if (delayMs) {
  console.log(`  delay_ms=${delayMs}`);
}
if (prefix) {
  console.log(`  prefix=${prefix}`);
}
if (suffix) {
  console.log(`  suffix=${suffix}`);
}

const stream = client.StreamProcess(request);

stream.on("data", (chunk) => {
  console.log("received chunk:", chunk.result ?? chunk);
});

stream.on("end", () => {
  console.log("stream completed");
});

stream.on("error", (err) => {
  console.error("stream error:", err);
  process.exitCode = 1;
});

process.on("SIGINT", () => {
  console.log("Interrupted, closing stream");
  stream.cancel();
});
