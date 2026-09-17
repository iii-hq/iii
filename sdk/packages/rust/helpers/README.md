# iii-helpers

Shared helper primitives across the iii SDKs.

OpenTelemetry types are re-exported at `iii_helpers::observability::opentelemetry`.
Use that path rather than depending on `opentelemetry` directly: the crate pins one
OpenTelemetry minor (0.31 through iii 0.24, 0.32 from the next release) and bumps it as a minor release.

See https://github.com/iii-hq/iii for the full project.
