import React, { useEffect, useRef, useState } from "react";
import { TerminalLog } from "../types";

interface TerminalProps {
  onClose: () => void;
  isGodMode?: boolean;
}

export const Terminal: React.FC<TerminalProps> = ({
  onClose,
  isGodMode = false,
}) => {
  const [logs, setLogs] = useState<TerminalLog[]>([]);
  const [input, setInput] = useState("");
  const endRef = useRef<HTMLDivElement>(null);
  const hasBooted = useRef(false);

  const addLog = (
    message: string,
    type: TerminalLog["type"] = "info",
    clickableCommand?: string,
  ) => {
    setLogs((prev) => [
      ...prev,
      {
        id: Date.now() + Math.random(),
        timestamp: new Date().toLocaleTimeString([], { hour12: false }),
        type,
        message,
        clickableCommand,
      },
    ]);
  };

  useEffect(() => {
    // Prevent double-run in React StrictMode during development
    if (hasBooted.current) return;
    hasBooted.current = true;

    const bootSequence = async () => {
      setLogs([]);
      if (isGodMode) {
        addLog("!!! ROOT ACCESS DETECTED !!!", "error");
        await new Promise((r) => setTimeout(r, 200));
        addLog("OVERRIDING BSL SAFETY LOCKS...", "glitch");
        await new Promise((r) => setTimeout(r, 400));
        addLog("ENGINE KERNEL: UNLOCKED", "success");
        await new Promise((r) => setTimeout(r, 200));
        addLog('Type "help" for available commands.', "info", "help");
        return;
      }

      addLog("Starting iii engine...", "system");
      await new Promise((r) => setTimeout(r, 300));
      addLog("Loading configuration from config.yaml...", "info");
      await new Promise((r) => setTimeout(r, 200));
      addLog("Initializing modules...", "info");
      await new Promise((r) => setTimeout(r, 150));
      addLog("  ✓ HttpModule listening on 127.0.0.1:3111", "success");
      await new Promise((r) => setTimeout(r, 150));
      addLog("  ✓ StreamModule listening on 127.0.0.1:3112", "success");
      await new Promise((r) => setTimeout(r, 150));
      addLog("  ✓ StateModule", "success");
      await new Promise((r) => setTimeout(r, 150));
      addLog("  ✓ QueueModule (Redis-backed)", "success");
      await new Promise((r) => setTimeout(r, 150));
      addLog("  ✓ PubSubModule", "success");
      await new Promise((r) => setTimeout(r, 150));
      addLog("  ✓ CronModule (Redis-backed)", "success");
      await new Promise((r) => setTimeout(r, 150));
      addLog("  ✓ OtelModule", "success");
      await new Promise((r) => setTimeout(r, 300));
      addLog("WebSocket server listening on ws://127.0.0.1:49134", "warning");
      await new Promise((r) => setTimeout(r, 200));
      addLog('Engine ready. Type "help" for commands.', "success", "help");
    };

    bootSequence();
  }, [isGodMode]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  const handleCommand = (cmd: string) => {
    const command = cmd.trim().toLowerCase();
    const args = command.split(" ").slice(1);
    const baseCommand = command.split(" ")[0];

    addLog(`> ${cmd}`, "info");

    switch (baseCommand) {
      case "help":
        addLog("═══ iii ENGINE COMMANDS ═══", "system");
        addLog("", "info");
        addLog("GETTING STARTED:", "warning");
        addLog("  install     - Installation instructions", "info", "install");
        addLog("  quickstart  - Get started guide", "info", "quickstart");
        addLog("  version     - Engine version info", "info", "version");
        addLog("", "info");
        addLog("ENGINE:", "warning");
        addLog("  status      - Check engine status", "info", "status");
        addLog("  modules     - List active modules", "info", "modules");
        addLog("  config      - Configuration example", "info", "config");
        addLog("  ports       - Show active ports", "info", "ports");
        addLog("", "info");
        addLog("DEVELOPMENT:", "warning");
        addLog("  sdk         - SDK installation & usage", "info", "sdk");
        addLog(
          "  register    - Function registration example",
          "info",
          "register",
        );
        addLog(
          "  triggers    - Trigger types (http, queue, cron, subscribe)",
          "info",
          "triggers",
        );
        addLog(
          "  protocol    - WebSocket protocol messages",
          "info",
          "protocol",
        );
        addLog("", "info");
        addLog("ARCHITECTURE:", "warning");
        addLog("  arch        - System architecture", "info", "arch");
        addLog("  adapters    - Adapter ecosystem", "info", "adapters");
        addLog("  workers     - Worker connection info", "info", "workers");
        addLog("  redis       - Redis integration", "info", "redis");
        addLog("", "info");
        addLog("SYSTEM:", "warning");
        addLog("  clear       - Clear console", "info", "clear");
        addLog("  exit        - Close terminal", "info", "exit");
        if (isGodMode) {
          addLog("", "info");
          addLog("🔴 ROOT ACCESS:", "error");
          addLog(
            "  internals   - Engine internals & architecture",
            "error",
            "internals",
          );
          addLog(
            "  roadmap     - Internal development roadmap",
            "error",
            "roadmap",
          );
          addLog(
            "  wake        - Universal wake-up mechanism",
            "error",
            "wake",
          );
          addLog(
            "  builtins    - Built-in host functions",
            "error",
            "builtins",
          );
          addLog(
            "  registry    - View internal registries state",
            "error",
            "registry",
          );
          addLog(
            "  perf        - Performance metrics & bottlenecks",
            "error",
            "perf",
          );
          addLog(
            "  debug       - Debug mode & tracing config",
            "error",
            "debug",
          );
          addLog(
            "  unsafe      - Unsafe operations & memory stats",
            "error",
            "unsafe",
          );
          addLog(
            "  kill-switch - BSL enforcement protocol",
            "error",
            "kill-switch",
          );
        }
        break;

      case "install":
        addLog("═══ INSTALLATION ═══", "system");
        addLog("", "info");
        addLog("macOS / Linux:", "warning");
        addLog("", "info");
        addLog(
          "curl -fsSL https://install.iii.dev/iii/main/install.sh | sh",
          "success",
        );
        addLog("", "info");
        addLog("Install specific version:", "warning");
        addLog("curl -fsSL [...]/install.sh | sh -s -- v0.6.3", "info");
        addLog("", "info");
        addLog("Custom install directory:", "warning");
        addLog("BIN_DIR=/usr/local/bin curl -fsSL [url] | sh", "info");
        addLog("", "info");
        addLog("Verify installation:", "warning");
        addLog("  command -v iii && iii --version", "success");
        break;

      case "quickstart":
        addLog("═══ QUICK START ═══", "system");
        addLog("", "info");
        addLog("1. Install the engine:", "warning");
        addLog(
          "   curl -fsSL https://install.iii.dev/iii/main/install.sh | sh",
          "info",
        );
        addLog("", "info");
        addLog("2. Start Redis (optional, for queue & cron):", "warning");
        addLog("   docker run -d -p 6379:6379 redis:alpine", "info");
        addLog("", "info");
        addLog("3. Run the engine:", "warning");
        addLog("   iii --config iii-config.yaml", "success");
        addLog("", "info");
        addLog("4. Connect a worker (Node.js):", "warning");
        addLog("   npm install iii-sdk", "info");
        addLog('   const iii = registerWorker("ws://127.0.0.1:49134")', "success");
        addLog('   iii.registerFunction({ id: "my.fn" }, handler)', "success");
        addLog("", "info");
        addLog("Read the docs at iii.dev/docs", "warning");
        break;

      case "status":
        addLog("═══ ENGINE STATUS ═══", "system");
        addLog("", "info");
        addLog("Engine:          ████ ONLINE", "success");
        addLog("WebSocket:       ws://127.0.0.1:49134", "success");
        addLog("Workers:         0 connected", "info");
        addLog("", "info");
        addLog("Active Modules:", "warning");
        addLog("  ✓ HttpModule (port 3111)", "success");
        addLog("  ✓ StreamModule (port 3112)", "success");
        addLog("  ✓ StateModule", "success");
        addLog("  ✓ QueueModule (Redis)", "success");
        addLog("  ✓ PubSubModule", "success");
        addLog("  ✓ CronModule (Redis)", "success");
        addLog("  ✓ OtelModule", "success");
        addLog("", "info");
        addLog("Triggers:        0 registered", "info");
        addLog("Functions:       0 registered", "info");
        addLog("Version:         v0.6.3", "warning");
        break;

      case "version":
        addLog("═══ VERSION INFO ═══", "system");
        addLog("", "info");
        addLog("Engine:     v0.6.3", "warning");
        addLog("SDK:        v0.3.0", "info");
        addLog("Edition:    Rust 2024", "info");
        addLog("Protocol:   WebSocket + JSON", "info");
        addLog("", "info");
        addLog("Check version:", "warning");
        addLog("  iii --version", "success");
        addLog("", "info");
        addLog("GitHub: github.com/iii-hq/iii", "info");
        break;

      case "ports":
        addLog("═══ ACTIVE PORTS ═══", "system");
        addLog("", "info");
        addLog("Worker WebSocket:", "warning");
        addLog("  ws://127.0.0.1:49134", "success");
        addLog("  (Workers connect here via SDK)", "info");
        addLog("", "info");
        addLog("HTTP API Module:", "warning");
        addLog("  http://127.0.0.1:3111", "success");
        addLog("  (HTTP triggers and endpoints)", "info");
        addLog("", "info");
        addLog("Stream Module:", "warning");
        addLog("  ws://127.0.0.1:3112", "success");
        addLog("  (Real-time subscriptions)", "info");
        addLog("", "info");
        addLog("Prometheus Metrics:", "warning");
        addLog("  http://127.0.0.1:9464", "success");
        addLog("  (OTel metrics endpoint)", "info");
        addLog("", "info");
        addLog("Redis (optional):", "warning");
        addLog("  redis://localhost:6379", "success");
        addLog("  (Queue and Cron modules)", "info");
        addLog("", "info");
        addLog("Ports are configurable in config.yaml", "warning");
        break;

      case "sdk":
        addLog("═══ SDK ═══", "system");
        addLog("", "info");
        addLog("Install:", "warning");
        addLog("  npm install iii-sdk", "success");
        addLog("  pip install iii-sdk", "success");
        addLog("  cargo add iii-sdk", "success");
        addLog("", "info");
        addLog("Node.js/TypeScript:", "warning");
        addLog('import { registerWorker } from "iii-sdk";', "success");
        addLog('const iii = registerWorker("ws://127.0.0.1:49134");', "success");
        addLog("", "info");
        addLog("iii.registerFunction({", "success");
        addLog('  id: "math.add"', "success");
        addLog("}, async (input) => {", "success");
        addLog("  return { sum: input.a + input.b };", "success");
        addLog("});", "success");
        addLog("", "info");
        addLog("Python:", "warning");
        addLog("from iii import register_worker", "success");
        addLog('iii = register_worker("ws://127.0.0.1:49134")', "success");
        addLog("", "info");
        addLog("Rust:", "warning");
        addLog('let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default())?;', "success");
        addLog("", "info");
        addLog('See "register" and "triggers" for more examples', "info");
        break;

      case "register":
        addLog("═══ FUNCTION REGISTRATION ═══", "system");
        addLog("", "info");
        addLog("Register a function:", "warning");
        addLog("", "info");
        addLog("iii.registerFunction({", "success");
        addLog('  id: "api.echo"', "success");
        addLog("}, async (req) => {", "success");
        addLog("  return {", "success");
        addLog("    status_code: 200,", "success");
        addLog("    body: { ok: true, input: req.body }", "success");
        addLog("  };", "success");
        addLog("});", "success");
        addLog("", "info");
        addLog("Function paths use double-colon notation:", "warning");
        addLog("  service::function", "info");
        addLog("  myapp::users::create", "info");
        addLog("  data::transform::json", "info");
        addLog("", "info");
        addLog('See "triggers" to expose functions', "warning");
        break;

      case "triggers":
        addLog("═══ TRIGGER TYPES ═══", "system");
        addLog("", "info");
        addLog("HTTP Trigger (endpoints):", "warning");
        addLog("", "info");
        addLog("iii.registerTrigger({", "success");
        addLog('  type: "http",', "success");
        addLog('  function_id: "api.echo",', "success");
        addLog("  config: {", "success");
        addLog('    api_path: "echo",', "success");
        addLog('    http_method: "POST"', "success");
        addLog("  }", "success");
        addLog("});", "success");
        addLog("", "info");
        addLog("→ http://127.0.0.1:3111/echo", "info");
        addLog("", "info");
        addLog("Queue Trigger (background jobs):", "warning");
        addLog('  type: "queue"', "info");
        addLog('  config: { topic: "user.created" }', "info");
        addLog("", "info");
        addLog("Subscribe Trigger (pub/sub events):", "warning");
        addLog('  type: "subscribe"', "info");
        addLog('  config: { topic: "order.placed" }', "info");
        addLog("", "info");
        addLog("Cron Trigger (scheduled):", "warning");
        addLog('  type: "cron"', "info");
        addLog('  config: { expression: "0 0 * * *" }', "info");
        addLog("", "info");
        addLog("State Trigger (on state change):", "warning");
        addLog('  type: "state"', "info");
        addLog('  config: { scope: "users", key: "*" }', "info");
        addLog("", "info");
        addLog("Stream Trigger (on stream update):", "warning");
        addLog('  type: "stream"', "info");
        addLog('  config: { stream_name: "cursors" }', "info");
        break;

      case "protocol":
        addLog("═══ WEBSOCKET PROTOCOL ═══", "system");
        addLog("", "info");
        addLog("Message types (JSON over WebSocket):", "warning");
        addLog("", "info");
        addLog("Worker → Engine:", "warning");
        addLog("  RegisterFunction", "success");
        addLog("  UnregisterFunction", "success");
        addLog("  RegisterTrigger", "success");
        addLog("  UnregisterTrigger", "success");
        addLog("  RegisterService", "success");
        addLog("  InvocationResult", "success");
        addLog("  Pong", "success");
        addLog("", "info");
        addLog("Engine → Worker:", "warning");
        addLog("  WorkerRegistered", "success");
        addLog("  InvokeFunction", "success");
        addLog("  TriggerRegistrationResult", "success");
        addLog("  FunctionsAvailable", "success");
        addLog("  Ping", "success");
        addLog("", "info");
        addLog("Invocations can be fire-and-forget", "warning");
        addLog("by omitting invocation_id", "info");
        addLog("", "info");
        addLog("See docs for full protocol spec", "info");
        break;

      case "workers":
        addLog("═══ WORKER CONNECTIONS ═══", "system");
        addLog("", "info");
        addLog("Workers connect via WebSocket:", "warning");
        addLog("  ws://127.0.0.1:49134", "success");
        addLog("", "info");
        addLog("Supported languages:", "warning");
        addLog("  ✓ Node.js / TypeScript (npm: iii-sdk)", "success");
        addLog("  ✓ Python (pip: iii-sdk)", "success");
        addLog("  ✓ Rust (cargo: iii-sdk)", "success");
        addLog("", "info");
        addLog("Worker lifecycle:", "warning");
        addLog("  1. Connect to engine", "info");
        addLog("  2. Register functions", "info");
        addLog("  3. Register triggers", "info");
        addLog("  4. Handle invocations", "info");
        addLog("  5. Return results", "info");
        addLog("", "info");
        addLog("Multiple workers can connect", "warning");
        addLog("to the same engine instance", "info");
        break;

      case "redis":
        addLog("═══ REDIS INTEGRATION ═══", "system");
        addLog("", "info");
        addLog("Required for:", "warning");
        addLog("  ✓ QueueModule (background jobs)", "success");
        addLog("  ✓ CronModule (scheduled tasks)", "success");
        addLog("  ○ StreamModule (optional, can use file/memory)", "info");
        addLog("", "info");
        addLog("Default connection:", "warning");
        addLog("  redis://localhost:6379", "success");
        addLog("", "info");
        addLog("Configure in iii-config.yaml:", "warning");
        addLog("", "info");
        addLog("- class: modules::queue::QueueModule", "success");
        addLog("  config:", "success");
        addLog("    adapter:", "success");
        addLog("      class: modules::queue::RedisAdapter", "success");
        addLog("      config:", "success");
        addLog("        redis_url: redis://localhost:6379", "success");
        addLog("", "info");
        addLog("Alternative: Run without Redis", "warning");
        addLog("by disabling Queue/Cron modules", "info");
        break;

      case "arch":
      case "architecture":
      case "diagram":
        addLog("═══ ARCHITECTURE ═══", "system");
        addLog("", "info");
        addLog("┌─────────────────┐", "warning");
        addLog("│  iii ENGINE     │ ← Rust Core", "warning");
        addLog("└───────┬─────────┘", "warning");
        addLog("        │", "info");
        addLog("        ▼", "info");
        addLog("┌─────────────────┐", "info");
        addLog("│ CORE MODULES    │", "info");
        addLog("├─────────────────┤", "info");
        addLog("│ • HTTP / Stream │", "success");
        addLog("│ • State / Queue │", "success");
        addLog("│ • PubSub / Cron │", "success");
        addLog("│ • OTel / Shell  │", "success");
        addLog("└───────┬─────────┘", "info");
        addLog("        │", "info");
        addLog("        ▼", "info");
        addLog("┌─────────────────┐", "info");
        addLog("│ ADAPTERS        │", "info");
        addLog("│ Redis│Postgres  │", "info");
        addLog("└───────┬─────────┘", "info");
        addLog("        │", "info");
        addLog("        ▼", "info");
        addLog("┌─────────────────┐", "success");
        addLog("│ WORKERS         │", "success");
        addLog("│ Node│Python│Rust│ ← Via SDK", "success");
        addLog("└─────────────────┘", "success");
        addLog("", "info");
        addLog("Flow: Engine → Modules → Adapters → Workers", "warning");
        break;

      case "modules":
        addLog("═══ CORE MODULES ═══", "system");
        addLog("All modules built with Rust", "warning");
        addLog("", "info");
        addLog("modules::api::RestApiModule (HTTP)", "success");
        addLog("  • HTTP triggers on host:port", "info");
        addLog("  • Default: 127.0.0.1:3111", "info");
        addLog("", "info");
        addLog("modules::stream::StreamModule", "success");
        addLog("  • Real-time state sync over WebSocket", "info");
        addLog("  • Default: 127.0.0.1:3112", "info");
        addLog("  • File/memory or Redis backed", "info");
        addLog("", "info");
        addLog("modules::state::StateModule", "success");
        addLog("  • Key-value state with triggers", "info");
        addLog("  • state::set, state::get, state::delete", "info");
        addLog("", "info");
        addLog("modules::queue::QueueModule", "success");
        addLog("  • Redis-backed background jobs", "info");
        addLog("  • Queue trigger + queue::enqueue", "info");
        addLog("", "info");
        addLog("modules::pubsub::PubSubModule", "success");
        addLog("  • Pub/sub event distribution", "info");
        addLog("  • Subscribe trigger + publish", "info");
        addLog("", "info");
        addLog("modules::cron::CronModule", "success");
        addLog("  • Cron-based scheduling", "info");
        addLog("  • Redis-backed distributed locks", "info");
        addLog("", "info");
        addLog("modules::observability::OtelModule", "success");
        addLog("  • OpenTelemetry traces, metrics, logs", "info");
        addLog("  • Prometheus metrics on :9464", "info");
        addLog("", "info");
        addLog("modules::shell::ExecModule", "success");
        addLog("  • File watcher + command execution", "info");
        addLog("  • Optional module", "info");
        addLog("", "info");
        addLog('Type "config" to see configuration', "warning");
        break;

      case "adapters":
        addLog("═══ ADAPTER ECOSYSTEM ═══", "system");
        addLog("", "info");
        addLog("StreamModule adapters:", "warning");
        addLog("  • modules::stream::adapters::KvStore", "success");
        addLog("    (file_based or in_memory)", "info");
        addLog("  • modules::stream::adapters::RedisAdapter", "success");
        addLog("", "info");
        addLog("QueueModule adapters:", "warning");
        addLog("  • modules::queue::RedisAdapter", "success");
        addLog("", "info");
        addLog("CronModule adapters:", "warning");
        addLog("  • modules::cron::RedisCronAdapter", "success");
        addLog("", "info");
        addLog("HttpModule:", "warning");
        addLog("  • Direct HTTP (no adapter needed)", "success");
        addLog("", "info");
        addLog('Configure in config.yaml (type "config")', "warning");
        break;

      case "config":
        addLog("═══ CONFIGURATION (iii-config.yaml) ═══", "system");
        addLog("", "info");
        addLog("modules:", "success");
        addLog("  - class: modules::api::RestApiModule", "success");
        addLog("    config:", "success");
        addLog("      port: 3111", "success");
        addLog("      host: 127.0.0.1", "success");
        addLog("", "info");
        addLog("  - class: modules::stream::StreamModule", "success");
        addLog("    config:", "success");
        addLog("      port: 3112", "success");
        addLog("      adapter:", "success");
        addLog("        class: modules::stream::adapters::KvStore", "success");
        addLog("        config:", "success");
        addLog("          store_method: file_based", "success");
        addLog("", "info");
        addLog("  - class: modules::queue::QueueModule", "success");
        addLog("    config:", "success");
        addLog("      adapter:", "success");
        addLog("        class: modules::queue::RedisAdapter", "success");
        addLog("        config:", "success");
        addLog("          redis_url: redis://localhost:6379", "success");
        addLog("", "info");
        addLog("Supports environment variable expansion:", "warning");
        addLog("  ${REDIS_URL:redis://localhost:6379}", "info");
        break;

      case "bridge":
        addLog("═══ SDK ═══", "system");
        addLog("", "info");
        addLog("// TypeScript Example", "warning");
        addLog("", "info");
        addLog('import { registerWorker } from "iii-sdk";', "success");
        addLog("", "info");
        addLog('const iii = registerWorker("ws://localhost:49134");', "success");
        addLog("", "info");
        addLog("iii.registerFunction({", "success");
        addLog('  id: "myService.greet",', "success");
        addLog("}, async (input) => {", "success");
        addLog("  return { message: `Hello, ${input.name}!` };", "success");
        addLog("});", "success");
        addLog("", "info");
        addLog("iii.registerTrigger({", "success");
        addLog('  type: "http",', "success");
        addLog('  function_id: "myService::greet",', "success");
        addLog(
          '  config: { api_path: "/greet", http_method: "POST" }',
          "success",
        );
        addLog("});", "success");
        addLog("", "info");
        addLog("All SDKs: iii-sdk (npm, pip, cargo)", "warning");
        break;

      case "trigger":
        addLog("═══ TRIGGER ═══", "system");
        addLog("", "info");
        addLog("// Synchronous invocation (wait for result)", "warning");
        addLog("const result = await iii.trigger({", "success");
        addLog('  function_id: "userService::getProfile",', "success");
        addLog('  payload: { userId: "123" }', "success");
        addLog("});", "success");
        addLog("", "info");
        addLog("// Async invocation (fire and forget)", "warning");
        addLog("iii.trigger({", "success");
        addLog('  function_id: "emailService::sendWelcome",', "success");
        addLog('  payload: { email: "user@example.com" },', "success");
        addLog("  action: TriggerAction.Void()", "success");
        addLog("});", "success");
        addLog("", "info");
        addLog(
          "Functions can trigger OTHER functions across workers!",
          "success",
        );
        addLog("Node.JS worker → Python worker? No problem.", "warning");
        break;

      case "compare":
        addLog("═══ iii vs OTHERS ═══", "system");
        addLog("", "info");
        addLog("vs Temporal:", "warning");
        addLog("  ✓ Both: Durable execution", "info");
        addLog("  ✓ iii: Single binary, real-time", "success");
        addLog("", "info");
        addLog("vs Dapr:", "warning");
        addLog("  ✓ Both: Language agnostic, modular", "info");
        addLog("  ✓ iii: Durable exec, Rust core", "success");
        addLog("", "info");
        addLog("vs Serverless:", "warning");
        addLog("  ✓ iii: Self-hosted, language agnostic", "success");
        addLog("  ✓ iii: Real-time streams", "success");
        addLog("", "info");
        addLog("iii = Unifies ALL patterns.", "warning");
        break;

      case "lang":
      case "polyglot": // Legacy alias
        addLog("═══ POLYGLOT ═══", "system");
        addLog("", "info");
        addLog("Write workers in ANY language:", "warning");
        addLog("", "info");
        addLog(
          "  ┌────────────────┬────────────────────────────────────┐",
          "info",
        );
        addLog(
          "  │ Language       │ SDK Package                        │",
          "info",
        );
        addLog(
          "  ├────────────────┼────────────────────────────────────┤",
          "info",
        );
        addLog(
          "  │ TypeScript/JS  │ npm install iii-sdk                │",
          "success",
        );
        addLog(
          "  │ Python         │ pip install iii-sdk                │",
          "success",
        );
        addLog(
          "  │ Rust           │ cargo add iii-sdk                  │",
          "success",
        );
        addLog(
          "  │ Go             │ Coming Soon                        │",
          "warning",
        );
        addLog(
          "  └────────────────┴────────────────────────────────────┘",
          "info",
        );
        addLog("", "info");
        addLog("A TypeScript worker can trigger a Python function.", "success");
        addLog("A Go service can trigger a Rust handler.", "success");
        addLog("", "info");
        addLog("The engine doesn't care. It just orchestrates.", "warning");
        break;

      case "durable":
        addLog("═══ DURABLE EXECUTION ═══", "system");
        addLog("", "info");
        addLog('What makes execution "durable"?', "warning");
        addLog("", "info");
        addLog(
          "  1. REPLAY: If a worker crashes, execution resumes",
          "success",
        );
        addLog("     from the last checkpoint, not from scratch.", "info");
        addLog("", "info");
        addLog(
          "  2. HISTORY: Every function invocation is logged. Debug",
          "success",
        );
        addLog("     any failure by replaying the exact sequence.", "info");
        addLog("", "info");
        addLog(
          "  3. ORDERING: Events processed in order, guaranteed.",
          "success",
        );
        addLog("     No race conditions, no lost messages.", "info");
        addLog("", "info");
        addLog(
          "  4. LONG-RUNNING: Functions can run for days/weeks.",
          "success",
        );
        addLog("     Wait for human input. Sleep. Resume.", "info");
        addLog("", "info");
        addLog('This is what separates iii from "serverless".', "warning");
        break;

      case "dual-mode":
        addLog("═══ DUAL-MODE ═══", "system");
        addLog("", "info");
        addLog("Your worker can be triggered TWO ways:", "warning");
        addLog("", "info");
        addLog("MODE A: MANAGED (Via Engine)", "success");
        addLog("  • Engine sends X-iii-Action: POKE header", "info");
        addLog("  • Worker wakes, opens WebSocket to Engine", "info");
        addLog("  • Receives task, executes, reports result", "info");
        addLog("  • Use case: Heavy workflows, coordinated tasks", "info");
        addLog("", "info");
        addLog("MODE B: DIRECT (Via HTTP)", "success");
        addLog("  • External service POSTs directly to worker URL", "info");
        addLog("  • Worker executes immediately, returns response", "info");
        addLog("  • Can STILL access engine for capabilities", "info");
        addLog("  • Use case: Webhooks, public APIs, fast endpoints", "info");
        addLog("", "info");
        addLog("Same code. Two entry points. Maximum flexibility.", "warning");
        break;

      case "fabric":
        addLog("═══ FABRIC ACCESS ═══", "system");
        addLog("", "info");
        addLog("Scenario: Cloudflare Worker needs GPU access", "warning");
        addLog("", "info");
        addLog("  User → POST /resize → Cloudflare Worker", "info");
        setTimeout(() => addLog("", "info"), 300);
        setTimeout(
          () => addLog('  Worker: "I need GPU capability..."', "info"),
          300,
        );
        setTimeout(
          () =>
            addLog("  Worker: Opening lazy WebSocket to Engine...", "system"),
          600,
        );
        setTimeout(
          () => addLog("  Engine: Routing to basement GPU server...", "system"),
          900,
        );
        setTimeout(
          () => addLog("  GPU Server: Processing image...", "glitch"),
          1200,
        );
        setTimeout(
          () =>
            addLog("  Worker: Received result, returning to user", "success"),
          1500,
        );
        setTimeout(() => {
          addLog("", "info");
          addLog(
            "Ephemeral workers can borrow HEAVY infrastructure.",
            "warning",
          );
          addLog("Public API powered by private GPU. Seamlessly.", "success");
        }, 1800);
        break;

      case "poke":
        if (args.length === 0) {
          addLog("Usage: poke <target_url>", "warning");
          addLog("", "info");
          addLog('The "poke" wakes up a sleeping/serverless worker.', "info");
          addLog("Engine sends HTTP with X-iii-Action: POKE header.", "info");
          addLog("Worker boots, connects WebSocket, awaits tasks.", "info");
        } else {
          addLog(`Poking ${args[0]}...`, "info");
          setTimeout(() => addLog("Sending X-iii-Action: POKE", "system"), 300);
          setTimeout(() => addLog("Target awakening...", "info"), 600);
          setTimeout(
            () => addLog("WebSocket connection established!", "success"),
            1000,
          );
          setTimeout(
            () => addLog("Worker is now ONLINE and awaiting tasks.", "success"),
            1300,
          );
        }
        break;

      case "whereis":
        addLog("═══ WHEREIS ═══", "system");
        addLog("", "info");
        addLog("ANYWHERE with a network connection:", "warning");
        addLog("", "info");
        addLog("  ☁️  Cloud Functions", "success");
        addLog("      AWS Lambda, Google Cloud Functions, Azure", "info");
        addLog("", "info");
        addLog("  ⚡  Edge Platforms", "success");
        addLog("      Cloudflare Workers, Vercel Edge, Deno Deploy", "info");
        addLog("", "info");
        addLog("  🖥️  Traditional Servers", "success");
        addLog("      EC2, GCE, DigitalOcean, bare metal", "info");
        addLog("", "info");
        addLog("  🏠  Your Basement", "success");
        addLog("      Raspberry Pi, home lab, GPU rig", "info");
        addLog("", "info");
        addLog("  🐳  Containers", "success");
        addLog("      Docker, Kubernetes, ECS, Cloud Run", "info");
        addLog("", "info");
        addLog(
          "The engine doesn't care where. It just orchestrates.",
          "warning",
        );
        break;

      case "rust":
        addLog("═══ WHY RUST? ═══", "system");
        addLog("", "info");
        addLog("The iii Engine is written in Rust because:", "warning");
        addLog("", "info");
        addLog("  🚀 SPEED", "success");
        addLog("     No garbage collector. Near-C performance.", "info");
        addLog("     Orchestration layer can't be a bottleneck.", "info");
        addLog("", "info");
        addLog("  💾 MEMORY EFFICIENCY", "success");
        addLog("     Single binary, minimal footprint.", "info");
        addLog("     Run on a Raspberry Pi or a 128-core server.", "info");
        addLog("", "info");
        addLog("  🔒 SAFETY", "success");
        addLog("     No null pointers, no data races.", "info");
        addLog("     The engine CANNOT crash from memory errors.", "info");
        addLog("", "info");
        addLog("  🌐 PORTABILITY", "success");
        addLog("     Compile once, run anywhere.", "info");
        addLog("     Linux, macOS, Windows. Native binaries.", "info");
        addLog("", "info");
        addLog(
          '"Fast languages shouldn\'t be bottlenecked by slow runtimes."',
          "warning",
        );
        break;

      case "credits":
        addLog("═══ CREDITS ═══", "system");
        addLog("", "info");
        addLog("iii", "success");
        addLog("Intelligent Invocation Interface", "success");
        addLog("", "info");
        addLog("I - Intelligent (The Daemon)", "info");
        addLog("I - Invocation (The Trigger)", "info");
        addLog("I - Interface (The SDK)", "info");
        addLog("", "info");
        addLog("Built with ❤️ and Rust", "warning");
        addLog("", "info");
        addLog('"Context-Aware Execution"', "success");
        addLog("", "info");
        addLog("© 2026 Motia LLC", "info");
        break;

      case "motia":
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("ACCESS RESTRICTED", "error");
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("", "info");
        addLog("Motia internal protocol detected.", "warning");
        setTimeout(() => addLog("Clearance Level 4 required.", "warning"), 400);
        setTimeout(() => addLog("Contact your administrator.", "info"), 800);
        break;

      case "faq":
        addLog("═══ FAQ ═══", "system");
        addLog("", "info");
        addLog("Q: What IS iii?", "warning");
        addLog("A: A universal runtime engine that unifies APIs,", "info");
        addLog("   background jobs, queues, streams, workflows,", "info");
        addLog("   and AI agents under ONE programming model.", "info");
        addLog("", "info");
        addLog("Q: Who is it for?", "warning");
        addLog("A: Framework builders, platform teams, and IDP", "info");
        addLog("   teams building internal developer platforms.", "info");
        addLog("", "info");
        addLog("Q: What problem does it solve?", "warning");
        addLog("A: Backend fragmentation. Instead of 5+ tools", "info");
        addLog("   (queue service, API gateway, workflow engine,", "info");
        addLog("   event bus, scheduler), you use ONE engine.", "info");
        addLog("", "info");
        addLog("Q: Best analogy?", "warning");
        addLog('A: "The Kernel of Distributed Systems"', "success");
        addLog('   "The V8 of Distributed Backends"', "success");
        addLog('   "LLVM for Cloud Compute"', "success");
        break;

      case "internals":
        if (!isGodMode) {
          addLog("ACCESS DENIED. ROOT AUTHORIZATION REQUIRED.", "error");
          addLog("(Hint: ↑↑↓↓←→←→BA)", "info");
          break;
        }
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("🔴 ENGINE INTERNALS", "error");
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("", "info");
        addLog("THE CORE IDEA:", "warning");
        addLog("iii is an engine enabling interoperability between", "info");
        addLog("different systems regardless of language. It has a", "info");
        addLog("protocol to communicate with workers.", "info");
        addLog("", "info");
        addLog("HUB-AND-SPOKE MODEL:", "warning");
        addLog("  Hub (Engine): Manages state, triggers, modules", "info");
        addLog("  Spokes (Workers): Execute business logic", "info");
        addLog("", "info");
        addLog("KEY REGISTRIES (Arc<RwLock>):", "warning");
        addLog("  • WorkerRegistry - Connected workers", "info");
        addLog("  • FunctionsRegistry - Registered capabilities", "info");
        addLog(
          "  • TriggerRegistry - HTTP/Queue/Cron/Subscribe/State/Stream",
          "info",
        );
        addLog("", "info");
        addLog("RUST BENEFITS:", "success");
        addLog("  • Memory efficient - no GC pauses", "info");
        addLog("  • Fast - no bottleneck for Go/Rust workers", "info");
        addLog("  • Single binary orchestrates everything", "info");
        addLog("", "info");
        addLog("CURRENT CONCERNS:", "warning");
        addLog("  • JSON serialization overhead (perf ok for now)", "info");
        addLog("  • Redis cron lock TTL (30s, no heartbeat yet)", "info");
        addLog("  • Considering binary serialization (MsgPack/Proto)", "info");
        break;

      case "wake":
        if (!isGodMode) {
          addLog("ACCESS DENIED. ROOT AUTHORIZATION REQUIRED.", "error");
          addLog("(Hint: ↑↑↓↓←→←→BA)", "info");
          break;
        }
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("🔴 UNIVERSAL WAKE-UP MECHANISM", "error");
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("", "info");
        addLog("THE INSIGHT:", "warning");
        addLog('Engine doesn\'t need "Ephemeral" or "Long-Living"', "info");
        addLog("concepts. Those are human distinctions.", "info");
        addLog("", "info");
        addLog("FROM ENGINE'S PERSPECTIVE:", "success");
        addLog("  • ONLINE: Active WebSocket connection", "info");
        addLog("  • OFFLINE: Has a URL to make one appear", "info");
        addLog("", "info");
        addLog("THE UNIVERSAL LOOP:", "warning");
        addLog("  1. Task arrives", "info");
        addLog("  2. Compatible worker connected? → Dispatch", "info");
        addLog("  3. No worker? Check registry for Trigger_URL", "info");
        addLog("     → YES: Poke the URL, wait for connect", "info");
        addLog("     → NO: Queue task (wait for human)", "info");
        addLog("", "info");
        addLog("HANDLES EVERYTHING:", "success");
        addLog("  • Serverless: Poke → Wake → Run → Die", "info");
        addLog("  • Local Server: Poke → Boot → Connect", "info");
        addLog("  • Autoscaling: Poke → Cloud decides how", "info");
        addLog("", "info");
        addLog("SDK decides lifecycle, NOT Engine.", "warning");
        break;

      case "builtins":
        if (!isGodMode) {
          addLog("ACCESS DENIED. ROOT AUTHORIZATION REQUIRED.", "error");
          addLog("(Hint: ↑↑↓↓←→←→BA)", "info");
          break;
        }
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("🔴 BUILT-IN HOST FUNCTIONS", "error");
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("", "info");
        addLog("HOST FUNCTIONS (registered by modules):", "warning");
        addLog("", "info");
        addLog("state::set / state::get / state::delete", "success");
        addLog("  Key-value state management", "info");
        addLog("  state::update, state::list, state::list_groups", "info");
        addLog("", "info");
        addLog("stream::set / stream::get / stream::delete", "success");
        addLog("  Real-time state with WebSocket sync", "info");
        addLog("  stream::list, stream::update, stream::list_groups", "info");
        addLog("", "info");
        addLog("queue::enqueue", "success");
        addLog("  Enqueue background job (Redis-backed)", "info");
        addLog("", "info");
        addLog("publish", "success");
        addLog("  Distribute event to all subscribers", "info");
        addLog("  iii.trigger({ function_id: 'publish', payload: { topic, data }, action: TriggerAction.Void() })", "info");
        addLog("", "info");
        addLog("engine::functions::list", "success");
        addLog("  List all registered functions", "info");
        addLog("", "info");
        addLog("engine::workers::list", "success");
        addLog("  List all connected workers", "info");
        addLog("", "info");
        addLog(
          "All invokable via iii.trigger({ function_id, payload })",
          "warning",
        );
        break;

      case "roadmap":
        if (!isGodMode) {
          addLog("ACCESS DENIED. ROOT AUTHORIZATION REQUIRED.", "error");
          addLog("(Hint: ↑↑↓↓←→←→BA)", "info");
          break;
        }
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("🔴 INTERNAL ROADMAP", "error");
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("", "info");
        addLog("ACTIVE DEVELOPMENT:", "success");
        addLog("  ✓ Built-in KV storage (PR #44)", "info");
        addLog("  ✓ New module registry system (PR #49)", "info");
        addLog("  ✓ Motia moved to separate repo (PR #50)", "info");
        addLog("", "info");
        addLog("PLANNED IMPROVEMENTS:", "warning");
        addLog("  • Binary serialization (MsgPack/Protobuf)", "info");
        addLog("  • Heartbeat for long-running cron jobs", "info");
        addLog("  • Bun Docker image support", "info");
        addLog("", "info");
        addLog("ARCHITECTURAL NOTES:", "warning");
        addLog("  • Engine struct uses Arc refs (watch for cycles)", "info");
        addLog("  • JSON serialization passed stress tests", "info");
        addLog("  • Redis SET NX PX for cron locks (30s TTL)", "info");
        addLog("", "info");
        addLog("TARGET USERS:", "success");
        addLog("  Framework developers building on iii", "info");
        addLog("  Motia users won't see iii directly", "info");
        addLog("  Core contributors only", "info");
        break;

      case "registry":
        if (!isGodMode) {
          addLog("ACCESS DENIED. ROOT AUTHORIZATION REQUIRED.", "error");
          addLog("(Hint: ↑↑↓↓←→←→BA)", "info");
          break;
        }
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("🔴 INTERNAL REGISTRIES", "error");
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("", "info");
        addLog("WORKER REGISTRY (Arc<RwLock<HashMap>>):", "warning");
        addLog(
          "  • Tracks: Worker ID, WebSocket channel, capabilities",
          "info",
        );
        addLog("  • Live workers: 0", "info");
        addLog("  • Pending invocations: Map<Uuid, oneshot::Sender>", "info");
        addLog("  • Health checks: Ping/Pong every 30s", "info");
        addLog("", "info");
        addLog("FUNCTIONS REGISTRY (Arc<RwLock<HashMap>>):", "warning");
        addLog("  • Stores: id -> Box<dyn FunctionHandler>", "info");
        addLog("  • Registered functions: 0", "info");
        addLog("  • Hash tracking for change detection", "info");
        addLog("  • Thread-safe Arc refs for concurrent access", "info");
        addLog("", "info");
        addLog("TRIGGER REGISTRY (Arc<RwLock<HashMap>>):", "warning");
        addLog("  • Maps: (type, id) -> Trigger struct", "info");
        addLog(
          "  • Types: http, queue, subscribe, cron, state, stream",
          "info",
        );
        addLog("  • Active triggers: 0", "info");
        addLog("  • Config stored as serde_json::Value", "info");
        break;

      case "perf":
        if (!isGodMode) {
          addLog("ACCESS DENIED. ROOT AUTHORIZATION REQUIRED.", "error");
          addLog("(Hint: ↑↑↓↓←→←→BA)", "info");
          break;
        }
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("🔴 PERFORMANCE METRICS", "error");
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("", "info");
        addLog("SERIALIZATION OVERHEAD:", "warning");
        addLog("  • JSON encode/decode: ~50-100μs per message", "info");
        addLog("  • Acceptable for most workloads", "info");
        addLog("  • Considering MsgPack/Protobuf for v2", "info");
        addLog("", "info");
        addLog("WEBSOCKET LATENCY:", "warning");
        addLog("  • Local (127.0.0.1): <1ms", "success");
        addLog("  • LAN: 1-5ms", "success");
        addLog("  • Tokio async runtime overhead: negligible", "info");
        addLog("", "info");
        addLog("MEMORY PROFILE:", "warning");
        addLog("  • Engine baseline: ~8-12MB", "success");
        addLog("  • Per worker: ~2-4MB overhead", "info");
        addLog("  • Redis connections pooled", "info");
        addLog("  • No GC pauses (Rust)", "success");
        addLog("", "info");
        addLog("KNOWN BOTTLENECKS:", "warning");
        addLog("  • JSON serialization (not critical yet)", "info");
        addLog("  • Redis roundtrip for cron locks (~1-2ms)", "info");
        addLog("  • WebSocket frame overhead (~12 bytes/msg)", "info");
        addLog("", "info");
        addLog("CONCURRENCY:", "success");
        addLog("  • HTTP API: 1024 concurrent requests (default)", "info");
        addLog("  • Workers: Unlimited (bounded by system)", "info");
        addLog("  • Tokio tasks scale to CPU cores", "info");
        break;

      case "debug":
        if (!isGodMode) {
          addLog("ACCESS DENIED. ROOT AUTHORIZATION REQUIRED.", "error");
          addLog("(Hint: ↑↑↓↓←→←→BA)", "info");
          break;
        }
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("🔴 DEBUG & TRACING", "error");
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("", "info");
        addLog("TRACING INFRASTRUCTURE:", "warning");
        addLog("  • Library: tracing + tracing-subscriber", "info");
        addLog("  • Levels: error, warn, info, debug, trace", "info");
        addLog("  • Current level: debug", "success");
        addLog("  • Format: colored, with timestamps", "info");
        addLog("", "info");
        addLog("LOG FIELDS:", "warning");
        addLog("  • worker_id: UUID of connected worker", "info");
        addLog("  • function_id: Function being invoked", "info");
        addLog("  • invocation_id: Unique invocation UUID", "info");
        addLog(
          "  • trigger_type: http/queue/cron/subscribe/state/stream",
          "info",
        );
        addLog("", "info");
        addLog("OTEL MODULE:", "warning");
        addLog("  • OpenTelemetry traces, metrics, logs", "info");
        addLog("  • Structured logs with serde_json", "info");
        addLog("  • Request ID propagation across workers", "info");
        addLog("", "info");
        addLog("ENABLE TRACE LOGGING:", "success");
        addLog("  RUST_LOG=trace iii --config config.yaml", "info");
        addLog("", "info");
        addLog("DEBUGGING TIPS:", "warning");
        addLog(
          '  • Check worker connections: Look for "Worker connected"',
          "info",
        );
        addLog(
          '  • Function registration: "Function registered: <path>"',
          "info",
        );
        addLog('  • Invocations: "Remembering invocation for worker"', "info");
        break;

      case "unsafe":
        if (!isGodMode) {
          addLog("ACCESS DENIED. ROOT AUTHORIZATION REQUIRED.", "error");
          addLog("(Hint: ↑↑↓↓←→←→BA)", "info");
          break;
        }
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("🔴 UNSAFE OPERATIONS", "error");
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("", "info");
        addLog("RUST SAFETY:", "success");
        addLog("  • Zero unsafe blocks in core engine", "success");
        addLog("  • Memory safety guaranteed by compiler", "success");
        addLog("  • No manual memory management", "success");
        addLog("  • Thread safety via Send + Sync traits", "success");
        addLog("", "info");
        addLog("POTENTIAL ISSUES:", "warning");
        addLog("  • Arc cycle detection: Manual review needed", "info");
        addLog("  • Panic in worker handler = function fails", "info");
        addLog("  • WebSocket backpressure = memory buildup", "info");
        addLog("  • Redis connection pool exhaustion", "info");
        addLog("", "info");
        addLog("MEMORY STATS (SIMULATED):", "warning");
        addLog("  • Heap allocated: ~12.4 MB", "info");
        addLog("  • Stack size: 2 MB (Tokio default)", "info");
        addLog("  • Open sockets: 3 (WS, HTTP API, Streams)", "info");
        addLog("  • Redis connections: 2 (events, cron)", "info");
        addLog("", "info");
        addLog("THREAD MODEL:", "warning");
        addLog("  • Tokio runtime: Multi-threaded (CPU cores)", "info");
        addLog("  • Work-stealing scheduler", "info");
        addLog("  • No thread-per-connection overhead", "success");
        addLog("", "info");
        addLog("DANGEROUS OPERATIONS (DISABLED):", "error");
        addLog("  • Force worker disconnect: Not exposed", "info");
        addLog("  • Hot code reload: Not supported", "info");
        addLog("  • Runtime config modification: File-based only", "info");
        break;

      case "kill-switch":
        if (!isGodMode) {
          addLog("ACCESS DENIED. ROOT AUTHORIZATION REQUIRED.", "error");
          addLog("(Hint: ↑↑↓↓←→←→BA)", "info");
          break;
        }
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        addLog("!!! BSL ENFORCEMENT PROTOCOL !!!", "error");
        addLog("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", "error");
        setTimeout(
          () => addLog("Scanning for competing services...", "glitch"),
          500,
        );
        setTimeout(
          () => addLog("Analyzing license compliance...", "glitch"),
          1000,
        );
        setTimeout(() => addLog("VIOLATION DETECTED.", "error"), 1500);
        setTimeout(() => addLog("TERMINATING LICENSE.", "error"), 2000);
        setTimeout(
          () =>
            addLog("[This is a demo. No actual enforcement occurred.]", "info"),
          2500,
        );
        break;

      case "clear":
        setLogs([]);
        break;

      case "exit":
        onClose();
        break;

      default:
        addLog(`Command not found: ${command}`, "error");
        addLog('Type "help" for available commands.', "info");
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      handleCommand(input);
      setInput("");
    }
  };

  const theme = isGodMode
    ? {
        border: "border-red-600",
        bg: "bg-red-950/20",
        header: "bg-red-900/40 border-red-600",
        text: "text-red-500",
        input: "text-red-500 placeholder-red-700",
        caret: "text-red-500",
      }
    : {
        border: "border-iii-medium",
        bg: "bg-black",
        header: "bg-iii-dark border-iii-medium",
        text: "text-iii-light",
        input: "text-white placeholder-iii-medium",
        caret: "text-iii-accent",
      };

  return (
    <div
      className={`fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-2 sm:p-4 ${isGodMode ? "animate-pulse-fast" : ""}`}
    >
      <div
        className={`w-full max-w-4xl h-[calc(100vh-1rem)] sm:h-auto ${theme.bg} border ${theme.border} shadow-2xl overflow-hidden relative transition-all duration-500 rounded-lg`}
      >
        <div
          className={`flex items-center justify-between px-3 sm:px-4 py-2 ${theme.header} border-b ${theme.border}`}
        >
          <div className="flex items-center gap-1.5 sm:gap-2">
            <div
              className="w-3 h-3 sm:w-3.5 sm:h-3.5 rounded-full bg-red-500 cursor-pointer hover:bg-red-400 transition-colors touch-manipulation"
              onClick={onClose}
            />
            <div className="w-3 h-3 sm:w-3.5 sm:h-3.5 rounded-full bg-yellow-500" />
            <div className="w-3 h-3 sm:w-3.5 sm:h-3.5 rounded-full bg-green-500" />
          </div>
          <span
            className={`text-[9px] sm:text-xs ${theme.text} font-mono uppercase tracking-wider truncate ml-2`}
          >
            {isGodMode ? (
              <>
                <span className="hidden sm:inline">
                  ROOT_ACCESS // ENGINE_CORE v0.6.3
                </span>
                <span className="sm:hidden">ROOT_ACCESS</span>
              </>
            ) : (
              <>
                <span className="hidden sm:inline">
                  iii_engine_debug_console v0.6.3
                </span>
                <span className="sm:hidden">iii_debug_console</span>
              </>
            )}
          </span>
        </div>

        <div className="h-[calc(100vh-8rem)] sm:h-[24rem] md:h-[32rem] lg:h-[36rem] overflow-y-auto p-2 sm:p-3 md:p-4 font-mono text-[10px] sm:text-xs md:text-sm space-y-0.5 sm:space-y-1 bg-black">
          {logs.map((log) => (
            <div key={log.id} className="flex">
              <span className="hidden md:inline text-iii-medium/60 shrink-0 text-[10px] md:text-xs mr-2 md:mr-3">
                [{log.timestamp}]
              </span>
              {log.clickableCommand ? (
                <span
                  onClick={() => {
                    setInput(log.clickableCommand!);
                    handleCommand(log.clickableCommand!);
                  }}
                  className={`break-all whitespace-pre-wrap leading-relaxed cursor-pointer hover:underline transition-colors inline-block touch-manipulation
                    ${log.type === "error" ? "text-red-400 font-bold hover:text-red-300" : ""}
                    ${log.type === "success" ? "text-iii-accent hover:text-iii-accent/80" : ""}
                    ${log.type === "warning" ? "text-amber-400 hover:text-amber-300" : ""}
                    ${log.type === "system" ? "text-cyan-400 hover:text-cyan-300" : ""}
                    ${log.type === "info" ? "text-gray-300 hover:text-gray-200" : ""}
                    ${log.type === "glitch" ? "text-red-400 animate-glitch font-bold bg-red-900/20" : ""}
                  `}
                >
                  {log.message}
                </span>
              ) : (
                <span
                  className={`break-all whitespace-pre-wrap leading-relaxed
                  ${log.type === "error" ? "text-red-400 font-bold" : ""}
                  ${log.type === "success" ? "text-iii-accent" : ""}
                  ${log.type === "warning" ? "text-amber-400" : ""}
                  ${log.type === "system" ? "text-cyan-400" : ""}
                  ${log.type === "info" ? "text-gray-300" : ""}
                  ${log.type === "glitch" ? "text-red-400 animate-glitch font-bold bg-red-900/20" : ""}
                `}
                >
                  {log.message}
                </span>
              )}
            </div>
          ))}
          <div ref={endRef} />
        </div>

        <div
          className={`p-2 sm:p-3 md:p-4 border-t ${theme.border} bg-iii-dark/80 flex items-center gap-1.5 sm:gap-2`}
        >
          <span className={`${theme.caret} font-bold text-sm sm:text-base`}>
            {">"}
          </span>
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            className={`flex-1 bg-transparent border-none outline-none ${theme.input} font-mono text-xs sm:text-sm`}
            placeholder="type 'help'"
            autoFocus
          />
        </div>
      </div>
    </div>
  );
};
