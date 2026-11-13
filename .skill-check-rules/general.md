# General rules

Cross-cutting authoring and content rules for iii.

## One primitive: the Function

iii has one primitive, the **Function**, and two operators: you **call** a function, or you
**observe** it (`observe(a, b)`: when function `a` runs, function `b` runs). Workers host functions.
A worker is not a second primitive, and an observer (the binding that `observe` makes) is not a
third.

- Name the Function as the primitive. Describe workers as what hosts functions, and observers as
  what `observe` makes.
- Do not present workers, functions, and observers (or triggers) as "three primitives", "four
  pieces", or a set of equal concepts. This applies to titles, headings, descriptions, and taglines.
- When a page must name the parts together, use "Functions: call and observe" or "Workers host
  Functions".

Examples:

- ✅ "one primitive, the Function: call it or observe it"
- ✅ "Workers host Functions"
- ❌ "three primitives: Worker, Function, Observer"
- ❌ "Workers. Functions. Observers."

### Order: workers, functions, observers

Whenever two or more of workers, functions, and observers appear together in prose, lists,
headings, examples, or diagrams, preserve their relative order:

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

- A single term on its own has no ordering question.
- Applies in any presentation: running prose, bulleted lists, table column order, section headings,
  diagram element order, code examples that introduce more than one primitive.
- Triggers are removed in 0.26: a trigger type is an observable function and a trigger is an
  observer. A page that names triggers to describe 0.25, or the move from them (an upgrading guide,
  the changelog), is correct; do not flag it on ordering or naming grounds. The same applies to
  archived versioned pages (`docs/<version>/`) that describe the model of their own release.

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

