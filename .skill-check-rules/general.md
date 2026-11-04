# General rules

Cross-cutting authoring and content rules for iii.

## Order: workers, functions, observers

iii's three primitives are **workers, functions, and observers**: workers host work, functions do
it, and observers make it reactive (`observe(a, b)`: when function `a` runs, function `b` runs).
Whenever two or more of these primitives appear together in prose, lists, headings, examples, or
diagrams, preserve their relative order:

- workers before functions
- workers before observers
- functions before observers

Examples:

- ✅ "workers, functions, and observers"
- ✅ "Worker, Function, Observer"
- ✅ "functions and observers" (subset; relative order preserved)
- ❌ "observers, functions, and workers" (full inversion)
- ❌ "observers and functions" (subset violating relative order)

Scope:

- A single primitive on its own has no ordering question.
- Applies in any presentation: running prose, bulleted lists, table column order, section headings,
  diagram element order, code examples that introduce more than one primitive.
- Triggers are removed in 0.26: a trigger type is an observable function and a trigger is an
  observer. A page that names triggers to describe 0.25, or the move from them (an upgrading guide,
  the changelog), is correct; do not flag it on ordering or naming grounds.

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

## "Telemetry" is ambiguous — always disambiguate

iii has **two** distinct telemetry surfaces. Most prose talks about one or the other, rarely
both, and conflating them is a recurring failure mode. Always make clear which one you mean.

- **OpenTelemetry / observability** — traces, metrics, logs, baggage, sampling, alerts, custom
  spans, exporters. Owned by the `iii-observability` worker. Governed by `OTEL_*` env vars
  (`OTEL_ENABLED`, `OTEL_EXPORTER_*`, etc.). This is what users wire up to Datadog, Honeycomb,
  Grafana, Jaeger, etc. When in doubt, call this **observability** or **OpenTelemetry**, not
  "telemetry".
- **iii-telemetry (anonymous usage)** — Amplitude analytics, heartbeat, anonymous device-ID
  management. Owned by the `iii-telemetry` worker. Governed by `III_TELEMETRY_ENABLED`. This is
  the opt-out anonymous usage stream, **not** OpenTelemetry.

Rules:

- Never use bare "telemetry" to refer to OpenTelemetry surfaces — use "observability" or
  "OpenTelemetry".
- Reserve the bare word "telemetry" for `iii-telemetry` (anonymous usage), or always qualify it
  ("OTel telemetry", "anonymous usage telemetry").
- SDK callouts about logger / spans / metrics point at **iii-observability**, never
  iii-telemetry.
- `OTEL_ENABLED` and `III_TELEMETRY_ENABLED` are independent — disabling one does not disable
  the other. `how-to/disable-telemetry.mdx` documents both distinctly.

The canonical disambiguation is in [`workers.md`](./workers.md) ("Logger and telemetry belong
to iii-observability"); this rule lifts it so it's not buried.

## No "backend software" or "backend engineering"

Do not use the terms "backend software" or "backend engineering" (or their hyphenated/spaced
variants). Prefer "software", "software engineering", or "system design" depending on context.

iii is software for building systems; the "backend" qualifier implies a frontend/backend split that
isn't meaningful at the engine level. Enforced by the `Terminology.BackendSoftware` Vale rule.

