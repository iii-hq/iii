# iii-exec

Execute shell commands as part of engine startup — building assets, running migrations, or starting long-lived processes.

## Sample Configuration

```yaml
- name: iii-exec
  config:
    exec:
      - cd frontend && npm install
      - cd frontend && npm run build
      - bun run --enable-source-maps index-production.js
```

With file watching:

```yaml
- name: iii-exec
  config:
    watch:
      - steps/**/*.{ts,js}
      - config/*.json
    exec:
      - cd frontend && npm install
      - cd frontend && npm run build
      - bun run --enable-source-maps index-production.js
```

## Configuration

| Field | Type | Description |
|---|---|---|
| `exec` | string[] | Required. Sequential pipeline of commands. Intermediate commands must exit `0` before the next starts. The final command is kept running as a long-lived process. |
| `watch` | string[] | Glob patterns that trigger a full pipeline restart on file change. |

Because each entry runs as a separate process, shell state is not shared between entries. Use `&&` to chain operations that must share a working directory:

```yaml
- cd path/to/project && npm run build
```

If an intermediate command exits with a non-zero code, the pipeline stops and remaining commands are skipped.

> **Note on `watch` pattern limitations:** Patterns match by root directory and file extension only — the filename portion is ignored. `src/**/*.test.ts` matches all `.ts` files under `src/`, not only test files. Use `**` to watch subdirectories recursively: `config/*.json` only watches the top level of `config/`, while `config/**/*.json` watches at any depth. Files without an extension (e.g., `Makefile`, shell scripts) are never matched.
