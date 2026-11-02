# AGENTS.md

You are working in the iii monorepo — a backend unification engine with three primitives: **Worker**, **Function**, **Observer**. The engine is Rust. SDKs exist for TypeScript (Node and browser), Python, Rust, and Go. All communicate over WebSocket.

## Commands

```bash
# Setup
pnpm install                     # JS/TS dependencies
cargo build --release            # Rust workspace

# Build
pnpm build                       # all JS/TS packages (Turborepo)
cargo build --release             # engine + Rust SDK

# Test
pnpm test                        # all JS/TS tests
cargo test                       # all Rust tests
cargo test -p iii                 # engine only
cargo test -p iii-sdk             # Rust SDK only
cd sdk/packages/python/iii && uv sync --extra dev && uv run pytest  # Python SDK

# Lint & Format
pnpm fmt                         # format JS/TS (Biome)
pnpm fmt:check                   # check without changes
pnpm lint                        # lint JS/TS
cargo fmt --all                   # format Rust
cargo clippy --workspace          # lint Rust

# Run
cargo run --release               # start engine (reads engine/config.yaml)
pnpm dev:docs                     # docs dev server (Mintlify)
pnpm dev:website                  # website dev server

# Cloud
iii cloud deploy --config <path>  # deploy to iii Cloud
iii cloud list                    # list deployments
iii cloud update <deployment-id>  # update a deployment
iii cloud delete <deployment-id>  # delete a deployment
```

## Project Map

```
engine/                          Rust engine — runtime, modules, protocol, CLI
sdk/packages/node/iii/           TypeScript SDK (npm: iii-sdk)
sdk/packages/node/iii-browser/   Browser SDK (npm: iii-browser-sdk)
sdk/packages/python/iii/         Python SDK (PyPI: iii-sdk)
sdk/packages/rust/iii/           Rust SDK (crates.io: iii-sdk)
sdk/packages/go/iii/             Go SDK (github.com/iii-hq/iii/sdk/packages/go/iii)
skills/                          8 agent skills (auto-discovered by SkillKit)
docs/                            Documentation site (Mintlify/MDX)
website/                         iii.dev website
website/presentations/           Tech-spec presentations site (iii.dev/tech-specs/) — SOP in its README
tech-specs/                      Markdown-only specs; frontmatter in each README.md lists it on the site
scripts/                         Build and CI scripts
```

**Workspaces:** `Cargo.toml` (Rust), `pnpm-workspace.yaml` (JS/TS), `turbo.json` (build orchestration).

## Docs (Mintlify/MDX)

- **`--` renders as an em dash.** Mintlify's smart typography turns a double
  hyphen into `—` in rendered text. Any command with a `--flag` (`--up`,
  `--template`, `--api-key`) shown outside a Markdown code span — for example in
  a JSX `<span>` — must be wrapped in a JSX string expression so it renders
  verbatim: `<span>{'iii compose --up'}</span>`. Inside plain Markdown backticks
  it is already safe.
- Vale runs in the Skill check workflow (`.github/workflows/skill-check.yml`,
  configured by `.skill-check.yaml`). On each pull request the
  `iii-hq/skills-and-validation` action renders the `.skill.md` artifact of every
  changed `docs/next` page and checks it with Vale, using the action's bundled
  `Terminology` and `Diataxis` styles (picked per page by the frontmatter
  `type`), next to a structure check and an AI review. Run the same check
  locally with `iii-skill-check verify docs/next/<page>.mdx --layers structure,vale`.
- Vale bans em dashes (`Terminology.EmDash`) and phrases like "out of the box"
  (`Terminology.SlopMagic`) in prose. Rewrite with commas, parentheses, periods,
  or colons, and name the explicit default.
- After editing a `docs/next/*.mdx`, regenerate its skill artifact with
  `iii-skill-render --write docs/next/<page>.mdx`. Versioned copies under
  `docs/<version>/` are out of skill-render scope, so bring changes forward to
  them (and their `.skill.md`) by hand.

## Boundaries

### Always

- Use `pnpm` (never `npm`) for JS/TS packages
- Use `cargo fmt --all` before committing Rust changes
- Use `pnpm fmt` before committing JS/TS changes
- Use `call` to invoke functions (`call_async` in Python, `client.Call` in Go, `iii call` in the CLI)
- Use leading slashes for the `path` arg of `http::on-request` observers: `/orders`, `/users/:id`
- Use `expression` for the schedule arg of `cron::on-tick` observers
- Use `::` separator for function IDs: `orders::validate`, `reports::daily-summary`
- Use `workspace:*` for internal pnpm package references
- Include `## When to Use` and `## Boundaries` sections in every SKILL.md
- Match SKILL.md `name` field to its directory name exactly

### Ask First

- Changes to public SDK APIs (npm/PyPI/crates.io surface)
- Changes to engine config schema (`engine/config.yaml`)
- Changes to CI/CD workflows (`.github/`)
- Adding new engine modules
- Modifying the WebSocket protocol between SDK and engine

### Never

- Commit secrets, API keys, or credentials
- Use `npm` instead of `pnpm`
- Push directly to `main`
- Change engine and CLI licensing (ELv2) or SDK licensing (Apache-2.0)
- Remove "When to Use" / "Boundaries" from SKILL.md files (SkillKit validates these)
- Use `cron` as an arg key for `cron::on-tick`: the cron worker reads `expression`
- Omit the leading slash on an `http::on-request` `path` arg: the http worker standard is `/path`

## Code Style

Workers register functions, call them with `call`, and make one function run when another runs with
`observe(source, target)`. HTTP routes, cron schedules, and queue subscriptions are observers of
`http::on-request`, `cron::on-tick`, and `queue::on-message`: `args` are for that function's worker,
and `filters` are for iii.

**Rust (engine + SDK):**
```rust
// Function IDs use :: separator
iii.register_function(
    "orders::validate",
    RegisterFunction::new(validate_order).description("Validate an incoming order"),
);

// Call a function and wait for its result
let result = iii
    .call(CallRequest::new("orders::validate", json!({ "sku": "A1", "qty": 2 })))
    .await?;

// HTTP route: observe http::on-request; `path` has a leading slash
iii.observe(
    Source::function("http::on-request")
        .args(json!({ "method": "POST", "path": "/orders/validate" })),
    "orders::validate",
);

// Cron schedule: observe cron::on-tick with `expression` (7-field: sec min hour dom month dow year)
iii.observe(
    Source::function("cron::on-tick").args(json!({ "expression": "0 0 9 * * * *" })),
    "reports::daily-summary",
);

// Observe a function: filters use explicit operators; the target is a registered function
iii.observe(
    Source::function("orders::create").filters(json!({ "input.total": { "$gte": 500 } })),
    "alerts::big-order",
);
```

**TypeScript (SDK):**
```typescript
// Call a function and wait for its result
const result = await iii.call({ function_id: 'orders::validate', payload: { sku: 'A1', qty: 2 } });

// Fire-and-forget
await iii.call({ function_id: 'audit::log', payload: entry, action: CallAction.Void() });

// When orders::create runs, alerts::big-order runs
iii.observe('orders::create', 'alerts::big-order');

// HTTP route: observe http::on-request; `path` has a leading slash
iii.observe(
  { function_id: 'http::on-request', args: { method: 'POST', path: '/orders/validate' } },
  'orders::validate',
);

// HTTP route with middleware chain
iii.observe(
  {
    function_id: 'http::on-request',
    args: {
      method: 'POST',
      path: '/orders/validate',
      middleware_function_ids: ['middleware::auth', 'middleware::rate-limit'],
    },
  },
  'orders::validate',
);

// Cron schedule: observe cron::on-tick with `expression`
iii.observe(
  { function_id: 'cron::on-tick', args: { expression: '0 0 9 * * * *' } },
  'reports::daily-summary',
);

// Observer with metadata (optional, passed to the target with each run)
iii.observe(
  { function_id: 'cron::on-tick', args: { expression: '0 0 9 * * * *' } },
  'reports::daily-summary',
  { metadata: { owner: 'billing-team', priority: 'high' } },
);

// Filters select calls (or an engine event such as engine::on-functions-changed)
iii.observe(
  { function_id: 'orders::create', filters: { 'input.total': { $gte: 500 } } },
  'alerts::big-order',
);

// Provide an event source: the handler returns the event, and the worker calls the function when the
// event happens. Hooks override iii's defaults for this function's observers.
iii.registerFunction('files::on-change', (change: FileChange) => change, {
  args_format: WatchArgsSchema,
  private: true,
})
  .onObserverAdded((obs) => watcher.retain(obs.args.path as string))
  .onObserverRemoved((obs) => watcher.release(obs.args.path as string))
  // each change goes to the observers of its path: keys that iii indexes
  .onObserverMatch({ observer: (obs) => obs.args.path as string, call: (call) => call.input.path });

watcher.on('change', (change) => iii.call({ function_id: 'files::on-change', payload: change }));
```

**Python (SDK):**
```python
# Call a function: call blocks, call_async is awaitable
result = iii.call({"function_id": "orders::validate", "payload": {"sku": "A1", "qty": 2}})
result = await iii.call_async({"function_id": "orders::validate", "payload": {"sku": "A1", "qty": 2}})

# Same patterns: leading slash on `path`, schedule in `expression`
iii.observe(
    {"function_id": "http::on-request", "args": {"method": "POST", "path": "/orders/validate"}},
    "orders::validate",
)
iii.observe(
    {"function_id": "cron::on-tick", "args": {"expression": "0 0 9 * * * *"}},
    "reports::daily-summary",
)

# Observe a function
iii.observe(
    {"function_id": "orders::create", "filters": {"input.total": {"$gte": 500}}},
    "alerts::big-order",
)
```

**Go (SDK):**
```go
// Call a function and wait for its result
result, err := client.Call(ctx, iii.CallRequest{
    FunctionID: "orders::validate",
    Payload:    json.RawMessage(`{"sku":"A1","qty":2}`),
})

// HTTP route: observe http::on-request; `path` has a leading slash
client.Observe(iii.Source{
    FunctionID: "http::on-request",
    Args:       iii.Args{"method": "POST", "path": "/orders/validate"},
}, "orders::validate")

// When orders::create runs, alerts::big-order runs
client.ObserveFunction("orders::create", "alerts::big-order")
```

## Skills

The `skills/` directory contains 8 agent skills auto-discovered by `npx skills add iii-hq/iii` and `npx skillkit install iii-hq/iii`: 7 iii-prefixed knowledge skills (catalog in `skills/SKILLS.md`) and the `presentation` tool skill. Code examples live in each `SKILL.md`, in TypeScript, Python, Rust, and Go where the concept is language-specific.

## Blog (agent knowledge base)

Published at https://iii.dev/blog/ — architecture posts and worked examples for coding agents. Each post is available as markdown:

- Index: https://iii.dev/blog/index.md
- Posts: https://iii.dev/blog/<slug>.md (every slug listed in the index)

Source lives in `website/src/content/blog/`. Also indexed in https://iii.dev/llms.txt and https://iii.dev/AGENTS.md.

## Licensing

- `engine/` (engine and CLI): Elastic License v2 (ELv2)
- Everything else: Apache-2.0
