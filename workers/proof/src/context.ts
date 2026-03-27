import { simpleGit, type SimpleGit } from "simple-git";
import type { ScanResult } from "./types.js";
import * as fs from "node:fs";
import * as path from "node:path";

const MAX_DIFF_CHARS = 50_000;
const MAX_FILES = 12;
const MAX_COMMITS = 5;

const SOURCE_EXTENSIONS = new Set([".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs", ".cjs"]);
const SKIP_DIRS = new Set(["node_modules", "dist", "build", ".git", ".next", "coverage", "__pycache__", ".cache"]);
const TEST_PATTERN = /\.(test|spec|e2e)\.[tj]sx?$|__tests__/;

export async function scanChanges(
  target: "unstaged" | "staged" | "branch" | "commit" = "unstaged",
  cwd?: string,
  mainBranch?: string,
  commitHash?: string,
): Promise<ScanResult> {
  const git: SimpleGit = simpleGit(cwd ?? process.cwd());

  let diff: string;
  let files: string[];
  let commits: Array<{ hash: string; subject: string }> = [];

  switch (target) {
    case "branch": {
      const main = mainBranch ?? (await detectMainBranch(git));
      diff = await git.diff([`${main}...HEAD`]);
      const summary = await git.diffSummary([`${main}...HEAD`]);
      files = summary.files.map((f) => f.file).slice(0, MAX_FILES);
      const log = await git.log({ from: main, to: "HEAD", maxCount: MAX_COMMITS });
      commits = log.all.map((c) => ({ hash: c.hash, subject: c.message.split("\n")[0] }));
      break;
    }
    case "commit": {
      const hash = commitHash ?? "HEAD";
      diff = await git.diff([`${hash}^..${hash}`]);
      const summary = await git.diffSummary([`${hash}^..${hash}`]);
      files = summary.files.map((f) => f.file).slice(0, MAX_FILES);
      const log = await git.log({ from: `${hash}^`, to: hash, maxCount: 1 });
      commits = log.all.map((c) => ({ hash: c.hash, subject: c.message.split("\n")[0] }));
      break;
    }
    case "staged": {
      diff = await git.diff(["--cached"]);
      const summary = await git.diffSummary(["--cached"]);
      files = summary.files.map((f) => f.file).slice(0, MAX_FILES);
      break;
    }
    default: {
      diff = await git.diff();
      const summary = await git.diffSummary();
      files = summary.files.map((f) => f.file).slice(0, MAX_FILES);
      break;
    }
  }

  if (!diff.trim()) {
    return { diff: "", files: [], commits: [], empty: true };
  }

  const truncatedDiff =
    diff.length > MAX_DIFF_CHARS
      ? diff.slice(0, MAX_DIFF_CHARS) + "\n... (truncated)"
      : diff;

  return { diff: truncatedDiff, files, commits, empty: false };
}

export type CoverageEntry = {
  path: string;
  testFiles: string[];
  covered: boolean;
};

export type CoverageReport = {
  entries: CoverageEntry[];
  coveredCount: number;
  totalCount: number;
  percent: number;
};

export async function analyzeTestCoverage(
  changedFiles: string[],
  cwd?: string,
): Promise<CoverageReport> {
  const root = cwd ?? process.cwd();
  const sourceFiles = changedFiles.filter(
    (f) => SOURCE_EXTENSIONS.has(path.extname(f)) && !TEST_PATTERN.test(f),
  );

  if (sourceFiles.length === 0) {
    return { entries: [], coveredCount: 0, totalCount: 0, percent: 100 };
  }

  const testFiles = await findTestFiles(root);
  const testImports = new Map<string, Set<string>>();

  for (const testFile of testFiles) {
    const imports = await extractImports(path.join(root, testFile));
    for (const imp of imports) {
      const resolved = resolveImportPath(imp, testFile, root);
      if (resolved) {
        if (!testImports.has(resolved)) testImports.set(resolved, new Set());
        testImports.get(resolved)!.add(testFile);
      }
    }
  }

  const entries: CoverageEntry[] = sourceFiles.map((f) => {
    const tests = testImports.get(f);
    return {
      path: f,
      testFiles: tests ? [...tests] : [],
      covered: !!tests && tests.size > 0,
    };
  });

  const coveredCount = entries.filter((e) => e.covered).length;
  return {
    entries,
    coveredCount,
    totalCount: entries.length,
    percent: entries.length > 0 ? Math.round((coveredCount / entries.length) * 100) : 100,
  };
}

async function findTestFiles(root: string, dir = "", results: string[] = []): Promise<string[]> {
  const fullDir = path.join(root, dir);
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(fullDir, { withFileTypes: true });
  } catch {
    return results;
  }

  for (const entry of entries) {
    if (SKIP_DIRS.has(entry.name)) continue;
    const rel = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (results.length < 200) await findTestFiles(root, rel, results);
    } else if (TEST_PATTERN.test(entry.name)) {
      results.push(rel);
    }
  }
  return results;
}

async function extractImports(filePath: string): Promise<string[]> {
  let content: string;
  try {
    content = fs.readFileSync(filePath, "utf-8");
  } catch {
    return [];
  }

  const imports: string[] = [];
  const importRe = /from\s+['"]([^'"]+)['"]/g;
  const requireRe = /require\s*\(\s*['"]([^'"]+)['"]\s*\)/g;

  let match: RegExpExecArray | null;
  while ((match = importRe.exec(content)) !== null) imports.push(match[1]);
  while ((match = requireRe.exec(content)) !== null) imports.push(match[1]);

  return imports.filter((i) => i.startsWith("."));
}

function resolveImportPath(importPath: string, fromFile: string, root: string): string | null {
  const fromDir = path.dirname(fromFile);
  const resolved = path.normalize(path.join(fromDir, importPath));

  for (const ext of ["", ".ts", ".tsx", ".js", ".jsx", "/index.ts", "/index.js"]) {
    const full = path.join(root, resolved + ext);
    try {
      if (fs.statSync(full).isFile()) return resolved + ext;
    } catch { /* not found */ }
  }
  return resolved;
}

async function detectMainBranch(git: SimpleGit): Promise<string> {
  try {
    const ref = await git.raw(["symbolic-ref", "refs/remotes/origin/HEAD"]);
    return ref.trim().replace("refs/remotes/origin/", "");
  } catch {
    return "main";
  }
}
