import type { Browser, BrowserContext, Page } from "playwright";

export type StepResult = {
  id: string;
  description: string;
  status: "running" | "passed" | "failed";
  assertions: Array<{ text: string; passed: boolean }>;
  startedAt: number;
  completedAt?: number;
};

export type RunReport = {
  runId: string;
  title: string;
  steps: StepResult[];
  status: "pass" | "fail" | "error";
  passRate: number;
  files: string[];
  startedAt: number;
  completedAt: number;
  recordedActions: Array<{ tool: string; input: Record<string, unknown> }>;
};

export type SavedFlow = {
  slug: string;
  title: string;
  baseUrl: string;
  actions: Array<{ tool: string; input: Record<string, unknown> }>;
  savedAt: number;
};

export type ScanResult = {
  diff: string;
  files: string[];
  commits: Array<{ hash: string; subject: string }>;
  empty: boolean;
};

export type RefEntry = {
  role: string;
  name: string;
  level?: number;
};

export type ConsoleEntry = {
  type: string;
  text: string;
  timestamp: number;
};

export type NetworkEntry = {
  method: string;
  url: string;
  status?: number;
  resourceType: string;
  timestamp: number;
};

export type BrowserSession = {
  browser: Browser;
  context: BrowserContext;
  page: Page;
  refMap: Map<string, RefEntry>;
  headed: boolean;
  consoleMessages: ConsoleEntry[];
  networkRequests: NetworkEntry[];
  replayEvents: unknown[];
  cdpUrl?: string;
};

export type RunInput = {
  target?: "unstaged" | "staged" | "branch" | "commit";
  main_branch?: string;
  commit_hash?: string;
  base_url?: string;
  instruction?: string;
  headed?: boolean;
  cwd?: string;
  cdp?: string;
  cookies?: boolean;
};
