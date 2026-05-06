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

## `motia-tools` is out of scope

`crates/motia-tools` (the `motia` binary) is a separate product surface that predates the iii
rename. Its docs are at motia.dev, not ideal-docs. When auditing the crates monorepo, do not
flag `motia-tools` as a missing docs target.
