# Quickstart

A quickstart example project that shows you how to scaffold a cross-language project, compose Python and TypeScript workers, and incrementally add functionality to a live system with zero downtime.

| Worker          | Language   | Function                | Does                                     |
| --------------- | ---------- | ----------------------- | ---------------------------------------- |
| `math-worker`   | Python     | `math::add`             | Returns `{ c: a + b }`                   |
| `caller-worker` | TypeScript | `math::add_two_numbers` | Calls `math::add` and returns the result |

Continue with the tutorial at: https://iii.dev/docs/quickstart
