#!/usr/bin/env node
// Convert HTML comments to MDX comments in *.mdx.skill.md files.
//
// iii-skill-render currently emits an HTML comment header ("<!-- DO NOT EDIT -->")
// and passes through any HTML comments in source partials. Mintlify's MDX parser
// rejects HTML comments, so it refuses to render those files. This script swaps
// `<!--` → `{/*` and `-->` → `*/}` in every *.mdx.skill.md under docs/, keeping
// the "do not edit" warning visible in source while letting Mintlify parse the
// file cleanly.
//
// Stopgap: the real fix is for iii-skill-render to emit MDX comments directly.

import { readdir, readFile, writeFile } from "node:fs/promises";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const DOCS_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

async function* walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name.startsWith(".")) continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) yield* walk(full);
    else if (entry.isFile() && full.endsWith(".mdx.skill.md")) yield full;
  }
}

let changed = 0;
for await (const file of walk(DOCS_ROOT)) {
  const original = await readFile(file, "utf8");
  const normalized = original.replaceAll("<!--", "{/*").replaceAll("-->", "*/}");
  if (normalized !== original) {
    await writeFile(file, normalized);
    changed += 1;
  }
}

console.log(`normalize-skill-comments: rewrote ${changed} file(s).`);
