import { registerWorker, Logger, TriggerAction } from "iii-sdk";
import { scanChanges, analyzeTestCoverage } from "./context.js";
import { runAgent } from "./agent.js";
import {
  launchBrowser, getSession, buildSnapshot, autoDiscoverCdp,
  handleNavigate, handleClick, handleType, handleSelect,
  handlePress, handleScreenshot, handleConsoleLogs,
  handleNetworkRequests, handlePerformanceMetrics,
  handlePlaywrightExec, closeBrowser, closeAll,
} from "./browser.js";
import { extractAndInjectCookies } from "./cookies.js";
import type { BrowserSession, RunInput, SavedFlow } from "./types.js";

const iii = registerWorker(process.env.III_URL ?? "ws://localhost:49134");
const logger = new Logger();

let activeRunId: string | null = null;

function acquireRun(runId: string): void {
  if (activeRunId) throw new Error("Another run is in progress. Wait or call proof::cleanup.");
  activeRunId = runId;
}

function releaseRun(): void {
  activeRunId = null;
}

function requireSession(): BrowserSession {
  if (!activeRunId) throw new Error("No active browser session. Call proof::run first.");
  const session = getSession(activeRunId);
  if (!session) throw new Error("No browser session");
  return session;
}

// ---------------------------------------------------------------------------
// Browser lifecycle — registered as iii functions
// ---------------------------------------------------------------------------

iii.registerFunction({ id: "proof::browser::launch" }, async (input) => {
  const { runId, headed, cdp } = input;
  acquireRun(runId);
  let cdpUrl: string | undefined;
  if (cdp === "auto") {
    cdpUrl = (await autoDiscoverCdp()) ?? undefined;
  } else if (cdp) {
    cdpUrl = cdp;
  }
  await launchBrowser(runId, headed, cdpUrl);
  logger.info("Browser launched", { runId, headed, cdp: cdpUrl ?? "none" });
  return { runId, launched: true };
});

iii.registerFunction({ id: "proof::browser::close" }, async (input) => {
  const result = await closeBrowser(input.runId);
  releaseRun();
  logger.info("Browser closed", { runId: input.runId });
  return result;
});

// ---------------------------------------------------------------------------
// Browser tools — 12 functions called by the agent via iii.trigger()
// ---------------------------------------------------------------------------

iii.registerFunction({ id: "proof::browser::navigate" }, async (input) =>
  handleNavigate(input.url, requireSession()));

iii.registerFunction({ id: "proof::browser::snapshot" }, async () => {
  const s = requireSession();
  return buildSnapshot(s.page, s.refMap);
});

iii.registerFunction({ id: "proof::browser::click" }, async (input) =>
  handleClick(input.ref, requireSession()));

iii.registerFunction({ id: "proof::browser::type" }, async (input) =>
  handleType(input.ref, input.text, requireSession()));

iii.registerFunction({ id: "proof::browser::select" }, async (input) =>
  handleSelect(input.ref, input.value, requireSession()));

iii.registerFunction({ id: "proof::browser::press" }, async (input) =>
  handlePress(input.ref, input.key, requireSession()));

iii.registerFunction({ id: "proof::browser::screenshot" }, async () =>
  handleScreenshot(requireSession()));

iii.registerFunction({ id: "proof::browser::assert" }, async (input) => {
  logger.info("Assertion", { assertion: input.assertion, passed: input.passed });
  return { assertion: input.assertion, passed: input.passed };
});

iii.registerFunction({ id: "proof::browser::console_logs" }, async (input) =>
  handleConsoleLogs(requireSession(), input));

iii.registerFunction({ id: "proof::browser::network" }, async (input) =>
  handleNetworkRequests(requireSession(), {
    method: input.method,
    urlContains: input.url_contains,
    resourceType: input.resource_type,
    clear: input.clear,
  }));

iii.registerFunction({ id: "proof::browser::performance" }, async () =>
  handlePerformanceMetrics(requireSession()));

iii.registerFunction({ id: "proof::browser::exec" }, async (input) =>
  handlePlaywrightExec(input.code, requireSession()));

iii.registerFunction({ id: "proof::cookies::inject" }, async (input) => {
  const session = requireSession();
  const count = await extractAndInjectCookies(session, input.url);
  logger.info("Cookies injected", { url: input.url, count });
  return { injected: count };
});

iii.registerFunction({ id: "proof::cdp::discover" }, async () => {
  const url = await autoDiscoverCdp();
  return { found: !!url, url };
});

// ---------------------------------------------------------------------------
// Pipeline functions — all inter-function calls go through iii.trigger()
// ---------------------------------------------------------------------------

iii.registerFunction({ id: "proof::scan" }, async (input) => {
  logger.info("Scanning changes", { target: input.target ?? "unstaged" });
  return scanChanges(input.target, input.cwd, input.main_branch, input.commit_hash);
});

iii.registerFunction({ id: "proof::coverage" }, async (input) => {
  logger.info("Analyzing test coverage", { files: input.files?.length });
  return analyzeTestCoverage(input.files ?? [], input.cwd);
});

iii.registerFunction({ id: "proof::execute" }, async (input) => {
  const { diff, files, base_url, instruction, runId, headed, commits, coverage, cdp, cookies } = input;
  logger.info("Executing agent loop", { runId, file_count: files?.length });

  await iii.trigger({
    function_id: "proof::browser::launch",
    payload: { runId, headed, cdp },
  });

  if (cookies) {
    await iii.trigger({
      function_id: "proof::cookies::inject",
      payload: { url: base_url },
    });
  }

  try {
    const trigger = iii.trigger.bind(iii);
    return await runAgent(trigger, diff, files, base_url, runId, instruction, commits, coverage);
  } finally {
    await iii.trigger({
      function_id: "proof::browser::close",
      payload: { runId },
    });
  }
});

iii.registerFunction({ id: "proof::report" }, async (input) => {
  const { report, scan } = input;
  logger.info("Test report", {
    status: report.status,
    pass_rate: `${report.passRate}%`,
    steps: report.steps.length,
  });

  await iii.trigger({
    function_id: "state::set",
    payload: { scope: "proof:reports", key: `report:${report.runId}`, data: report },
  });

  if (report.status === "pass" && report.steps.length > 0) {
    const base = report.title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "")
      .slice(0, 50);
    const slug = `${base}-${Date.now().toString(36)}`;

    const flow: SavedFlow = {
      slug,
      title: report.title,
      baseUrl: scan?.base_url ?? "",
      actions: report.recordedActions ?? [],
      savedAt: Date.now(),
    };

    await iii.trigger({
      function_id: "state::set",
      payload: { scope: "proof:flows", key: slug, data: flow },
    });
    logger.info("Flow saved", { slug });
  }

  await iii.trigger({
    function_id: "stream::set",
    payload: {
      stream_name: "proof",
      group_id: "results",
      item_id: report.runId,
      data: { status: report.status, title: report.title, passRate: report.passRate, completedAt: report.completedAt },
    },
  }).catch(() => {});

  return report;
});

iii.registerFunction({ id: "proof::run" }, async (input: RunInput) => {
  const runId = `run-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  const baseUrl = input.base_url ?? "http://localhost:3000";
  logger.info("Starting proof run", { runId, target: input.target ?? "unstaged" });

  const scan = await iii.trigger({
    function_id: "proof::scan",
    payload: { target: input.target, cwd: input.cwd, main_branch: input.main_branch, commit_hash: input.commit_hash },
  }) as Awaited<ReturnType<typeof scanChanges>>;

  if (scan.empty) {
    logger.info("No changes detected");
    return { status: "skip", reason: "No changes detected" };
  }

  const coverage = await iii.trigger({
    function_id: "proof::coverage",
    payload: { files: scan.files, cwd: input.cwd },
  });

  const report = await iii.trigger({
    function_id: "proof::execute",
    payload: {
      diff: scan.diff, files: scan.files, base_url: baseUrl,
      instruction: input.instruction, runId, headed: input.headed,
      commits: scan.commits, coverage, cdp: input.cdp, cookies: input.cookies,
    },
  });

  return iii.trigger({
    function_id: "proof::report",
    payload: { report, scan: { ...scan, base_url: baseUrl } },
  });
});

// ---------------------------------------------------------------------------
// Flow replay — all browser calls through iii.trigger()
// ---------------------------------------------------------------------------

iii.registerFunction({ id: "proof::replay" }, async (input) => {
  const { slug } = input;
  const flow = (await iii.trigger({
    function_id: "state::get",
    payload: { scope: "proof:flows", key: slug },
  })) as SavedFlow | null;

  if (!flow) return { status: "error", reason: `Flow "${slug}" not found` };

  logger.info("Replaying flow", { slug, actions: flow.actions.length });
  const runId = `replay-${Date.now()}`;

  await iii.trigger({
    function_id: "proof::browser::launch",
    payload: { runId, headed: input.headed ?? false },
  });

  const results: Array<{ tool: string; status: string; error?: string }> = [];

  try {
    for (const action of flow.actions) {
      try {
        await iii.trigger({
          function_id: `proof::browser::${action.tool.replace("browser_", "")}`,
          payload: action.input,
        });
        results.push({ tool: action.tool, status: "pass" });
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err);
        results.push({ tool: action.tool, status: "fail", error: msg });
      }
    }
  } finally {
    await iii.trigger({
      function_id: "proof::browser::close",
      payload: { runId },
    });
  }

  const failed = results.filter((r) => r.status === "fail").length;
  return { slug, status: failed === 0 ? "pass" : "fail", total: results.length, failed, results };
});

// ---------------------------------------------------------------------------
// State queries — all through iii.trigger()
// ---------------------------------------------------------------------------

iii.registerFunction({ id: "proof::flows" }, async () => {
  return iii.trigger({ function_id: "state::list", payload: { scope: "proof:flows" } });
});

iii.registerFunction({ id: "proof::history" }, async (input) => {
  const reports = await iii.trigger({ function_id: "state::list", payload: { scope: "proof:reports" } }) as any[];
  if (!Array.isArray(reports)) return [];
  return reports
    .sort((a: any, b: any) => (b.completedAt ?? 0) - (a.completedAt ?? 0))
    .slice(0, input?.limit ?? 20)
    .map((r: any) => ({
      runId: r.runId, title: r.title, status: r.status,
      passRate: r.passRate, steps: r.steps?.length ?? 0, completedAt: r.completedAt,
    }));
});

iii.registerFunction({ id: "proof::cleanup" }, async () => {
  await closeAll();
  releaseRun();
  logger.info("All browsers closed");
  return { cleaned: true };
});

// ---------------------------------------------------------------------------
// Queue-based runs — iii primitive, Expect can't do this
// ---------------------------------------------------------------------------

iii.registerFunction({ id: "proof::enqueue" }, async (input: RunInput) => {
  return iii.trigger({
    function_id: "proof::run",
    payload: input,
    action: TriggerAction.Enqueue({ queue: "proof" }),
  });
});

// ---------------------------------------------------------------------------
// HTTP triggers — every function accessible via REST
// ---------------------------------------------------------------------------

iii.registerTrigger({ type: "http", function_id: "proof::run", config: { api_path: "/proof", http_method: "POST" } });
iii.registerTrigger({ type: "http", function_id: "proof::replay", config: { api_path: "/proof/replay", http_method: "POST" } });
iii.registerTrigger({ type: "http", function_id: "proof::flows", config: { api_path: "/proof/flows", http_method: "GET" } });
iii.registerTrigger({ type: "http", function_id: "proof::history", config: { api_path: "/proof/history", http_method: "GET" } });
iii.registerTrigger({ type: "http", function_id: "proof::cleanup", config: { api_path: "/proof/cleanup", http_method: "POST" } });
iii.registerTrigger({ type: "http", function_id: "proof::coverage", config: { api_path: "/proof/coverage", http_method: "POST" } });
iii.registerTrigger({ type: "http", function_id: "proof::enqueue", config: { api_path: "/proof/enqueue", http_method: "POST" } });
iii.registerTrigger({ type: "http", function_id: "proof::cdp::discover", config: { api_path: "/proof/cdp", http_method: "GET" } });

console.log("proof worker started — listening for calls");
