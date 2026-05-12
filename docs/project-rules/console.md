# Console rules

Rules for documenting the iii Console — the standalone web UI that connects to a running engine.

## The Console is its own surface

The Console is **not** a worker. It's a standalone application that connects to a running iii
engine. Its documentation is in the iii docs (`using-iii/console.mdx`), not Worker Docs.

## Per-page content is console-owned

Each Console UI page (Workers, Functions, Triggers, States, Streams, Queues, Traces, Logs, Flow,
Config) renders data from a specific worker, but the _UI documentation_ is console-owned.

Don't push Console UI docs into worker pages — the user is reading "how do I use the Console's
States page" not "how does iii-state work." Conversely, don't push worker mechanics into the Console
doc; link or reference the worker.

## Flow page is opt-in

The Flow visualization is feature-flagged. Always note that it requires `--enable-flow` or
`III_ENABLE_FLOW`.

## Default ports

The Console assumes:

- `127.0.0.1:3111` — engine HTTP API
- `127.0.0.1:3112` — engine WebSocket
- `127.0.0.1:3113` — Console UI itself
- `127.0.0.1:49134` — SDK bridge WebSocket

These ports cross both the iii docs and Worker Docs surfaces; document them on the Console page
(since the Console is the consumer) and let the workers reference back.
