# SDK reference rules

Rules for the `sdk-reference/` pages (Node, Python, Rust, Browser, Engine).

## Strip worker-owned surfaces, add a callout

When a surface is owned by a specific worker, strip it from the SDK reference page and replace with a callout pointing to that worker's docs. See [`workers.md`](./workers.md) for which surfaces are worker-owned.

The callout convention used at the top of each client SDK page:

```mdx
<Note>
  Logging and telemetry surfaces ... are documented with the [iii-observability worker](#) — they live there because that worker owns logging and telemetry end-to-end.

  Channel surfaces (`ChannelReader`, `ChannelWriter`, `StreamChannelRef`, `createChannel`) are documented with the [iii-worker-manager](#) — channels are managed by that worker end-to-end.
</Note>
```

Adjust per language (e.g., `create_channel` in Python/Rust). If the SDK doesn't ship one of these surfaces (browser SDK has no OTel today), say so explicitly in the callout.

## Auto-reconnect / re-registration is SDK behavior

WebSocket reconnection with re-registration on disconnect is implemented by every current SDK (Node, Python, Rust, browser). Document it on each SDK's "Connection lifecycle" stub on the SDK reference page, not on `understanding-iii/workers.mdx`.

## Initialization variable convention

When showing or describing initialization, name the variable `worker`, not `iii`, so calls like `worker.registerFunction()` read naturally.

## Auto-generation expectations

SDK reference pages will eventually be auto-generated from code. The generator captures the surface — methods, types, signatures — but not narrative ("why use this SDK," migration guidance, when-to-pick-this-SDK).

Each SDK reference page should support a hand-written prefix (above auto-gen content) and/or suffix (below) — as a frontmatter slot, separate `_prefix.mdx`/`_suffix.mdx` partials, or a leading paragraph the generator preserves. Use MDX comment markers like `{/* TODO(skills/auto-gen): ... */}` to flag positions where hand-authored narrative should land.

## Cross-SDK alignment

When stubbing a new method or type on one SDK, compare against the others (Node ↔ Python ↔ Rust ↔ Browser) before applying. Real differences exist (Rust's `trigger` is async natively, so no `trigger_async`; browser SDK doesn't ship OTel) but doc-level naming or omissions should be consistent.

Mark genuine cross-SDK shape differences in the page itself (e.g., `_This differs from Node, is this okay?_`) so they get a second look.
