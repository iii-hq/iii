# proof

AI-powered browser testing for the [iii engine](https://github.com/iii-hq/iii). Scans your code changes, launches a real browser, and verifies everything works.

proof registers browser tools as iii functions. Any agent connected to the engine — Claude Code, Codex, or the Anthropic API — can drive Chromium through snapshot-driven accessibility testing. No fragile CSS selectors. The AI reads the page structure, picks elements by ref, and acts.

## Quick Start

```bash
# Terminal 1: Start iii engine
iii --use-default-config

# Terminal 2: Start proof worker
cd workers/proof
npm install
npm run dev
```

proof registers 25 functions with the engine. You're ready to test.

## Usage

### Interactive (Claude Code / Codex)

With proof running, tell your agent:

> "Test my changes at localhost:3000"

The agent calls proof's browser functions through iii — no API key needed.

Or call functions directly:

```bash
# Scan for changes
iii trigger --function-id='proof::scan' \
  --payload='{"target":"unstaged","cwd":"/path/to/repo"}'

# Launch browser
iii trigger --function-id='proof::browser::launch' \
  --payload='{"runId":"test-1","headed":true}'

# Navigate
iii trigger --function-id='proof::browser::navigate' \
  --payload='{"url":"http://localhost:3000"}'

# Snapshot — get accessibility tree with [ref=eN] markers
iii trigger --function-id='proof::browser::snapshot' --payload='{}'

# Click by ref
iii trigger --function-id='proof::browser::click' --payload='{"ref":"e3"}'

# Type into input
iii trigger --function-id='proof::browser::type' \
  --payload='{"ref":"e1","text":"user@example.com"}'

# Screenshot
iii trigger --function-id='proof::browser::screenshot' --payload='{}'

# Check console errors
iii trigger --function-id='proof::browser::console_logs' --payload='{}'

# Check network requests
iii trigger --function-id='proof::browser::network' --payload='{}'

# Performance metrics (FCP, TTFB, CLS)
iii trigger --function-id='proof::browser::performance' --payload='{}'

# Raw Playwright execution
iii trigger --function-id='proof::browser::exec' \
  --payload='{"code":"return await page.title()"}'

# Close browser
iii trigger --function-id='proof::browser::close' --payload='{"runId":"test-1"}'
```

### Automated (CI / API)

For headless runs without an agent, proof drives Claude directly via the Anthropic API:

```bash
ANTHROPIC_API_KEY=sk-... npm run dev
```

```bash
# Full pipeline: scan → plan → execute → report
curl -X POST localhost:3111/proof \
  -H 'Content-Type: application/json' \
  -d '{"target":"branch","base_url":"http://localhost:3000"}'

# Queue-based run with auto-retry (uses iii Queue + DLQ)
curl -X POST localhost:3111/proof/enqueue \
  -d '{"target":"branch","base_url":"https://staging.myapp.com"}'
```

### Replay Saved Flows

Successful runs save as replayable flows — no AI needed for reruns:

```bash
# List saved flows
curl localhost:3111/proof/flows

# Replay a flow
curl -X POST localhost:3111/proof/replay \
  -d '{"slug":"login-flow-m1abc","headed":true}'

# Run history
curl localhost:3111/proof/history
```

## How It Works

```
proof::scan          git diff → changed files, commits
    ↓
proof::coverage      import graph → which files lack tests
    ↓
proof::execute       agent loop with browser tools
    ↓                  ↕ proof::browser::navigate
    ↓                  ↕ proof::browser::snapshot
    ↓                  ↕ proof::browser::click
    ↓                  ↕ proof::browser::type
    ↓                  ↕ proof::browser::screenshot
    ↓                  ↕ proof::browser::assert
    ↓
proof::report        results → iii State + Stream
```

The snapshot-driven approach:

1. `proof::browser::snapshot` returns an ARIA accessibility tree with `[ref=eN]` markers on every interactive element
2. The agent reads the tree, identifies elements by ref — not CSS selectors
3. `proof::browser::click`, `proof::browser::type` etc. resolve refs to Playwright locators
4. After each action, a fresh snapshot is returned with updated refs

This makes tests resilient to UI changes. Refs are structural, not visual.

## Input Options

```json
{
  "target": "unstaged | staged | branch | commit",
  "base_url": "http://localhost:3000",
  "instruction": "test the login flow",
  "headed": true,
  "cookies": true,
  "cdp": "auto",
  "cwd": "/path/to/repo",
  "commit_hash": "abc123",
  "main_branch": "main"
}
```

| Field | Default | Description |
|-------|---------|-------------|
| `target` | `unstaged` | What to scan: unstaged, staged, branch, or single commit |
| `base_url` | `http://localhost:3000` | URL of the app to test |
| `instruction` | — | Natural language instruction for what to test |
| `headed` | `false` | Show browser window |
| `cookies` | `false` | Extract and inject cookies from local Chrome/Firefox |
| `cdp` | — | CDP WebSocket URL or `"auto"` to discover running Chrome |
| `cwd` | worker cwd | Path to the git repository |
| `commit_hash` | `HEAD` | Specific commit hash (when target is `commit`) |

## Functions

### Browser Tools (12)

| Function | Description |
|----------|-------------|
| `proof::browser::launch` | Launch Chromium (headed or headless, CDP optional) |
| `proof::browser::close` | Close browser session |
| `proof::browser::navigate` | Navigate to URL, return snapshot |
| `proof::browser::snapshot` | ARIA accessibility tree with `[ref=eN]` markers |
| `proof::browser::click` | Click element by ref |
| `proof::browser::type` | Type text into input by ref |
| `proof::browser::select` | Select dropdown option by ref |
| `proof::browser::press` | Press keyboard key on element |
| `proof::browser::screenshot` | Capture page as base64 PNG |
| `proof::browser::console_logs` | Read browser console messages |
| `proof::browser::network` | Read network request log |
| `proof::browser::performance` | Core Web Vitals (FCP, TTFB, CLS) |
| `proof::browser::exec` | Execute raw Playwright code |
| `proof::browser::assert` | Record a pass/fail assertion |

### Pipeline (10)

| Function | Description |
|----------|-------------|
| `proof::scan` | Git diff scanning (4 target modes) |
| `proof::coverage` | Import graph analysis → test coverage |
| `proof::execute` | Agent loop with Claude API |
| `proof::report` | Results → iii State + Stream |
| `proof::run` | Full pipeline orchestration |
| `proof::replay` | Replay a saved flow without AI |
| `proof::flows` | List saved flows |
| `proof::history` | Run history with trends |
| `proof::enqueue` | Queue-based run with retries + DLQ |
| `proof::cleanup` | Close all browser sessions |
| `proof::cookies::inject` | Extract local browser cookies |
| `proof::cdp::discover` | Find running Chrome CDP endpoint |

### HTTP Endpoints (8)

| Method | Path | Function |
|--------|------|----------|
| POST | `/proof` | `proof::run` |
| POST | `/proof/enqueue` | `proof::enqueue` |
| POST | `/proof/replay` | `proof::replay` |
| POST | `/proof/coverage` | `proof::coverage` |
| POST | `/proof/cleanup` | `proof::cleanup` |
| GET | `/proof/flows` | `proof::flows` |
| GET | `/proof/history` | `proof::history` |
| GET | `/proof/cdp` | `proof::cdp::discover` |

## iii Primitives Used

| Primitive | How proof uses it |
|-----------|------------------|
| **Functions** | 25 registered — browser tools, pipeline, queries |
| **Triggers** | 8 HTTP endpoints for REST access |
| **State** | Reports persisted to `proof:reports`, flows to `proof:flows` |
| **Streams** | Real-time test progress pushed to `proof` stream |
| **Queue** | `proof::enqueue` for CI runs with auto-retry |
| **DLQ** | Failed test runs land in DLQ for inspection |
| **Logger** | Every action traced with OTel |

## Architecture

```
┌──────────────────────────────────────────┐
│              iii Engine                   │
│         (ports 49134, 3111)              │
└──────────────────┬───────────────────────┘
                   │
          ┌────────┴────────┐
          │  proof worker   │
          │                 │
          │  25 functions   │
          │  8 HTTP routes  │
          │  Playwright     │
          │  simple-git     │
          └─────────────────┘
                   │
     ┌─────────────┼─────────────┐
     │             │             │
  Claude Code    Codex     Anthropic API
  (interactive)  (interactive)  (CI/automated)
```

Any agent on the engine can call proof's functions. The worker handles browser lifecycle, snapshot generation, and session management. The agent handles test logic.

## License

Apache-2.0
