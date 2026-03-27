import { chromium, type Browser, type Page } from "playwright";
import type { BrowserSession, ConsoleEntry, NetworkEntry, RefEntry } from "./types.js";

const INTERACTIVE_ROLES = new Set([
  "button", "link", "textbox", "checkbox", "radio", "combobox",
  "menuitem", "tab", "switch", "slider", "spinbutton", "searchbox",
]);

const CONTENT_ROLES = new Set([
  "heading", "img", "cell", "row", "alert", "status", "banner",
]);

const sessions = new Map<string, BrowserSession>();
let sharedBrowser: Browser | null = null;

async function getOrCreateBrowser(): Promise<Browser> {
  if (!sharedBrowser || !sharedBrowser.isConnected()) {
    sharedBrowser = await chromium.launch({ headless: true });
  }
  return sharedBrowser;
}

export async function autoDiscoverCdp(): Promise<string | null> {
  const endpoints = [
    "http://localhost:9222/json/version",
    "http://127.0.0.1:9222/json/version",
  ];
  for (const url of endpoints) {
    try {
      const res = await fetch(url, { signal: AbortSignal.timeout(2000) });
      const data = await res.json() as { webSocketDebuggerUrl?: string };
      if (data.webSocketDebuggerUrl) return data.webSocketDebuggerUrl;
    } catch { /* not running */ }
  }
  return null;
}

function setupPageTracking(page: Page, session: BrowserSession): void {
  page.on("console", (msg) => {
    session.consoleMessages.push({
      type: msg.type(),
      text: msg.text(),
      timestamp: Date.now(),
    });
  });

  page.on("response", (response) => {
    session.networkRequests.push({
      method: response.request().method(),
      url: response.url(),
      status: response.status(),
      resourceType: response.request().resourceType(),
      timestamp: Date.now(),
    });
  });
}

export async function launchBrowser(
  runId: string,
  headed = false,
  cdpUrl?: string,
): Promise<BrowserSession> {
  const existing = sessions.get(runId);
  if (existing) return existing;

  let browser: Browser;
  if (cdpUrl) {
    browser = await chromium.connectOverCDP(cdpUrl);
  } else if (headed) {
    browser = await chromium.launch({ headless: false });
  } else {
    browser = await getOrCreateBrowser();
  }

  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
  });
  const page = await context.newPage();

  const session: BrowserSession = {
    browser,
    context,
    page,
    refMap: new Map(),
    headed,
    consoleMessages: [],
    networkRequests: [],
    replayEvents: [],
    cdpUrl,
  };

  setupPageTracking(page, session);
  sessions.set(runId, session);
  return session;
}

export function getSession(runId: string): BrowserSession | undefined {
  return sessions.get(runId);
}

const ARIA_LINE_RE = /^(\s*)- (\w+)(?: "([^"]*)")?(.*)$/;

export async function buildSnapshot(
  page: Page,
  refMap: Map<string, RefEntry>,
): Promise<string> {
  const ariaSnapshot = await page.locator("body").ariaSnapshot();
  if (!ariaSnapshot) return "(empty page)";

  refMap.clear();
  let refCounter = 0;
  const outputLines: string[] = [];

  for (const line of ariaSnapshot.split("\n")) {
    const match = ARIA_LINE_RE.exec(line);
    if (!match) {
      outputLines.push(line);
      continue;
    }

    const [, indent, role, name, rest] = match;
    const isInteractive = INTERACTIVE_ROLES.has(role);
    const isContent = CONTENT_ROLES.has(role) && (name?.length ?? 0) > 0;

    let outputLine = `${indent}- ${role}`;
    if (name) outputLine += ` "${name}"`;
    if (rest) outputLine += rest;

    if (isInteractive || isContent) {
      refCounter++;
      const ref = `e${refCounter}`;
      outputLine += ` [ref=${ref}]`;
      refMap.set(ref, { role, name: name ?? "" });
    }

    outputLines.push(outputLine);
  }

  return outputLines.join("\n");
}

export function resolveRef(
  ref: string,
  refMap: Map<string, RefEntry>,
  page: Page,
) {
  const entry = refMap.get(ref);
  if (!entry) throw new Error(`Ref "${ref}" not found in current snapshot. Take a new snapshot.`);
  return page.getByRole(entry.role as any, { name: entry.name }).first();
}

export async function handleNavigate(url: string, session: BrowserSession): Promise<string> {
  await session.page.goto(url, { waitUntil: "domcontentloaded", timeout: 15_000 });
  return buildSnapshot(session.page, session.refMap);
}

export async function handleClick(ref: string, session: BrowserSession): Promise<string> {
  const locator = resolveRef(ref, session.refMap, session.page);
  await locator.click({ timeout: 10_000 });
  await session.page.waitForTimeout(300);
  return buildSnapshot(session.page, session.refMap);
}

export async function handleType(ref: string, text: string, session: BrowserSession): Promise<string> {
  const locator = resolveRef(ref, session.refMap, session.page);
  await locator.fill(text, { timeout: 10_000 });
  return buildSnapshot(session.page, session.refMap);
}

export async function handleSelect(ref: string, value: string, session: BrowserSession): Promise<string> {
  const locator = resolveRef(ref, session.refMap, session.page);
  await locator.selectOption(value, { timeout: 10_000 });
  return buildSnapshot(session.page, session.refMap);
}

export async function handlePress(ref: string, key: string, session: BrowserSession): Promise<string> {
  const locator = resolveRef(ref, session.refMap, session.page);
  await locator.press(key, { timeout: 10_000 });
  await session.page.waitForTimeout(300);
  return buildSnapshot(session.page, session.refMap);
}

export async function handleScreenshot(session: BrowserSession): Promise<string> {
  const buffer = await session.page.screenshot({ type: "png" });
  return buffer.toString("base64");
}

export async function handleConsoleLogs(
  session: BrowserSession,
  filter?: { type?: string; clear?: boolean },
): Promise<ConsoleEntry[]> {
  let logs = session.consoleMessages;
  if (filter?.type) {
    logs = logs.filter((l) => l.type === filter.type);
  }
  if (filter?.clear) {
    session.consoleMessages = [];
  }
  return logs;
}

export async function handleNetworkRequests(
  session: BrowserSession,
  filter?: { method?: string; urlContains?: string; resourceType?: string; clear?: boolean },
): Promise<NetworkEntry[]> {
  let reqs = session.networkRequests;
  if (filter?.method) reqs = reqs.filter((r) => r.method === filter.method);
  if (filter?.urlContains) reqs = reqs.filter((r) => r.url.includes(filter.urlContains!));
  if (filter?.resourceType) reqs = reqs.filter((r) => r.resourceType === filter.resourceType);
  if (filter?.clear) {
    session.networkRequests = [];
  }
  return reqs;
}

export async function handlePerformanceMetrics(session: BrowserSession) {
  return session.page.evaluate(() => {
    const perf = performance.getEntriesByType("navigation")[0] as PerformanceNavigationTiming | undefined;
    const paint = performance.getEntriesByType("paint");
    const fcp = paint.find((e) => e.name === "first-contentful-paint");

    const cls = (performance as any).getEntriesByType?.("layout-shift") ?? [];
    const clsValue = cls.reduce((sum: number, e: any) => sum + (e.hadRecentInput ? 0 : e.value), 0);

    return {
      url: location.href,
      fcp: fcp ? Math.round(fcp.startTime) : null,
      domContentLoaded: perf ? Math.round(perf.domContentLoadedEventEnd - perf.startTime) : null,
      load: perf ? Math.round(perf.loadEventEnd - perf.startTime) : null,
      ttfb: perf ? Math.round(perf.responseStart - perf.requestStart) : null,
      cls: Math.round(clsValue * 1000) / 1000,
      transferSize: perf?.transferSize ?? null,
    };
  });
}

export async function handlePlaywrightExec(
  code: string,
  session: BrowserSession,
): Promise<unknown> {
  const { page, context, browser } = session;
  const ref = (id: string) => {
    const entry = session.refMap.get(id);
    if (!entry) throw new Error(`Ref "${id}" not found`);
    return page.getByRole(entry.role as any, { name: entry.name }).first();
  };
  const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
  const fn = new AsyncFunction("page", "context", "browser", "ref", code);
  return fn(page, context, browser, ref);
}

export async function closeBrowser(runId: string): Promise<{ replayEvents: unknown[] }> {
  const session = sessions.get(runId);
  if (!session) return { replayEvents: [] };

  const events = session.replayEvents;
  await session.context.close();
  if (session.headed && session.browser !== sharedBrowser && !session.cdpUrl) {
    await session.browser.close();
  }
  sessions.delete(runId);
  return { replayEvents: events };
}

export async function closeAll(): Promise<void> {
  for (const [runId] of sessions) {
    await closeBrowser(runId);
  }
  if (sharedBrowser) {
    await sharedBrowser.close();
    sharedBrowser = null;
  }
}
