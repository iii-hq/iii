# Source

These rules start from `content/project-rules/` of iii-hq/skills-and-validation 0.4.32, the bundle
the `Verify docs skills` job installs. `.skill-check.yaml` points `rules.path` here so the AI layer
reviews iii's docs against the CLI, files, and ports this repository ships.

Files that differ from the bundle:

- `cli.md`: the commands of this CLI (`iii call` with its `trigger` and `t` aliases, `iii compose`
  with `--up`, `build` and `logs`, `iii ps`, `iii kill`), the removed `iii worker` and `iii console`
  commands with their replacements, Compose tooling as iii-level, and every finding emitted as a
  warning.
- `general.md`: the primitives in 0.26 order (workers, functions, observers), with pages about
  0.25 triggers left alone.
- `workers.md`: the Move to Worker Docs flag is always emitted as a warning, and the sandbox
  contract's S-codes stay on `creating-workers/sandboxes.mdx`, where their `docs_url` points.
- `config.md`: the compose file `worker-compose.yaml`, its lock, and configuration entry files.
- `console.md`: the Console as ADE and the default ports; the Flow page flag rule is dropped, since
  neither ADE nor the docs have that flag.

The other files are the bundle's, unchanged. The loader skips this file, `README.md`, and files whose
name starts with `_` (the system prompt `_skill-check-prompt.md` is read on its own).
