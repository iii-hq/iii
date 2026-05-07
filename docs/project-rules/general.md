# General rules

Cross-cutting authoring and content rules for iii.

## Adapters are deprecated

The default recommendation for adapter content is **remove**, or salvage the concept into a future
framing if it is broadly useful.

When you encounter `# adapter:` config blocks or per-worker adapter sections in source material, do
not migrate them. Drop or flag for the relevant Worker Docs.

## No "steps"

Do not refer to coding or process terms within iii workers or docs as a "step" or "steps". The
generic term such as in a tutorial's "step 1", "step 2" is okay.

## Record reader-facing changes in the changelog

When a change is something a docs reader would want to know about — a new page or section, a renamed
concept, removed or relocated content, or a correction to documented behavior — add a short entry to
[`changelog/index.mdx`](../changelog/index.mdx) as part of the same change.

Skip the changelog for everything else: typos, wording tweaks, formatting, link fixes, internal
tooling, project rules, Vale styles, and other repo plumbing. When in doubt, skip it — the changelog
is for readers, not contributors.

## Avoid dead code

Do not let unused exports, unused dependencies, or orphaned files / modules accumulate in any iii
repo (engine, sdk, console, cli, crates). When removing a feature, remove its code, its
dependencies, its config blocks, and any docs that reference it in the same change — do not leave
deprecated-but-still-shipped surfaces behind.

Each repo is expected to wire language-appropriate detection into CI (e.g., `cargo machete` and
`#![warn(dead_code)]` for Rust, `knip` for TypeScript, `vulture` / `ruff` / `deptry` for Python).
Tracker: [`todo/WORK.dead-code.md`](../todo/WORK.dead-code.md).

## No "backend software" or "backend engineering"

Do not use the terms "backend software" or "backend engineering" (or their hyphenated/spaced
variants). Prefer "software", "software engineering", or "system design" depending on context.

iii is software for building systems; the "backend" qualifier implies a frontend/backend split that
isn't meaningful at the engine level. Enforced by the `Terminology.BackendSoftware` Vale rule.

## `motia-tools` is out of scope

`crates/motia-tools` (the `motia` binary) is a separate product surface that predates the iii
rename. Its docs are at motia.dev, not ideal-docs. When auditing the crates monorepo, do not flag
`motia-tools` as a missing docs target.
