# TODOS

## State And Stream Updates

### Add nested path traversal for update operations

**What:** Support nested update paths such as `user.name` for `set`, `append`, `increment`, `decrement`, and `remove`.

**Why:** SDK comments used to imply dotted paths were nested, and users naturally expect `user.name` to update `{ "user": { "name": ... } }` instead of a first-level field named `user.name`.

**Context:** Atomic update paths currently target first-level field names only. Before implementing this, define compatibility behavior for literal dots in field names, escaping, and any array-index policy.

**Effort:** M
**Priority:** P2
**Depends on:** Append operation support and current path-semantics docs landing first.

### Add structured update warnings

**What:** Add machine-readable warning metadata for skipped update operations.

**Why:** Today, incompatible operations no-op and log a warning, but SDK callers cannot programmatically tell which operation was skipped.

**Context:** Keep `UpdateResult` backward-compatible for append support. A later API revision can extend the result shape or add a versioned warning channel after cross-SDK compatibility is designed.

**Effort:** M
**Priority:** P2
**Depends on:** Public API compatibility decision for `UpdateResult`.

### Audit legacy update-operation parity across adapters

**What:** Normalize existing non-append update behavior across the built-in KV store and Redis adapters.

**Why:** Some legacy scalar-root edge cases differ between Rust in-process logic and Redis Lua logic. Users should not get different results just because they switch adapters.

**Context:** Append support should add parity for the new operation without silently changing existing `set`, `increment`, or `decrement` behavior. This follow-up needs compatibility tests and a release-note decision because current Redis behavior may already be observable.

**Effort:** M
**Priority:** P2
**Depends on:** Compatibility decision for existing Redis update semantics.
