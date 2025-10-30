import path from "node:path";
import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";

const PROTO_PATH = path.join(process.cwd(), "proto", "engine.proto");
const pkgDef = await protoLoader.load(PROTO_PATH, {
  keepCase: false,
  longs: String,
  enums: String,
  defaults: true,
  oneofs: true,
});
const engine = grpc.loadPackageDefinition(pkgDef).engine.v1;

const client = new engine.Engine(
  "localhost:50051",
  grpc.credentials.createInsecure()
);

const renderValue = (value) => {
  if (!value) {
    return "unspecified";
  }

  if (value.stringValue !== undefined) {
    return value.stringValue || "";
  }
  if (value.numberValue !== undefined) {
    return String(value.numberValue);
  }
  if (value.boolValue !== undefined) {
    return String(value.boolValue);
  }
  if (value.nullValue !== undefined) {
    return "null";
  }
  if (value.listValue) {
    const items = (value.listValue.values || []).map(renderValue).join(", ");
    return `[${items}]`;
  }
  if (value.structValue) {
    const entries = Object.entries(value.structValue.fields || {})
      .map(([key, val]) => `${key}: ${renderValue(val)}`)
      .join(", ");
    return `{${entries}}`;
  }

  return "unspecified";
};

client.ListServices({}, (err, res) => {
  if (err) {
    console.error("ListServices error:", err);
    return;
  }

  const services = (res.services || []).map((svc) => ({
    name: svc.name,
    address: svc.address,
    serviceType: svc.serviceType,
    methods: (svc.methods || []).map((m) => ({
      name: m.name,
      kind: m.kind,
      description: m.description,
      requestFormat: renderValue(m.requestFormat),
      responseFormat: renderValue(m.responseFormat),
    })),
  }));

  console.log("Registered services:");
  services.forEach((svc) => {
    console.log(
      `  - ${svc.name} (${svc.serviceType || "unspecified"}) @ ${svc.address}`
    );
    svc.methods.forEach((method) => {
      console.log(`      * ${method.name} [${method.kind}]`);
      if (method.description) {
        console.log(`          description: ${method.description}`);
      }
      console.log(
        `          request: ${method.requestFormat || "unspecified"}`
      );
      console.log(
        `          response: ${method.responseFormat || "unspecified"}`
      );
    });
  });
});

const unaryRequest = {
  service: "text-formatter",
  method: "format_text",
  payload: "hello from node",
  meta: { prefix: "[", suffix: "]" },
};

client.Process(unaryRequest, (err, res) => {
  if (err) {
    console.error("Process error:", err);
  } else {
    console.log("Process response:", res); // → { result: "[HELLO FROM NODE]" }
  }
});

const streamRequest = {
  service: "text-streamer",
  method: "stream_chunks",
  payload: "streaming hello from node",
  meta: { chunk_size: "5", delay_ms: "200" },
};

const stream = client.StreamProcess(streamRequest);
stream.on("data", (chunk) => {
  console.log("Stream chunk:", chunk);
});
stream.on("end", () => {
  console.log("Stream completed");
});
stream.on("error", (err) => {
  console.error("Stream error:", err);
});
