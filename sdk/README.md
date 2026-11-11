# iii SDKs

The SDKs connect a program to the [iii engine](../engine/README.md) as a worker. A worker registers functions, calls functions, and observes functions. Every SDK has the same API and speaks the same JSON-over-WebSocket protocol.

## Packages

| Language | Package | Install | Source |
| --- | --- | --- | --- |
| TypeScript (Node.js) | [![npm](https://img.shields.io/npm/v/iii-sdk?label=iii-sdk)](https://www.npmjs.com/package/iii-sdk) | `pnpm add iii-sdk` | [`packages/node/iii`](packages/node/iii/README.md) |
| TypeScript (browser) | [![npm](https://img.shields.io/npm/v/iii-browser-sdk?label=iii-browser-sdk)](https://www.npmjs.com/package/iii-browser-sdk) | `pnpm add iii-browser-sdk` | [`packages/node/iii-browser`](packages/node/iii-browser/README.md) |
| Python | [![PyPI](https://img.shields.io/pypi/v/iii-sdk?label=iii-sdk)](https://pypi.org/project/iii-sdk/) | `pip install iii-sdk` | [`packages/python/iii`](packages/python/iii/README.md) |
| Rust | [![crates.io](https://img.shields.io/crates/v/iii-sdk?label=iii-sdk)](https://crates.io/crates/iii-sdk) | `cargo add iii-sdk` | [`packages/rust/iii`](packages/rust/iii/README.md) |
| Go | [![Go Reference](https://pkg.go.dev/badge/github.com/iii-hq/iii/sdk/packages/go/iii.svg)](https://pkg.go.dev/github.com/iii-hq/iii/sdk/packages/go/iii) | `go get github.com/iii-hq/iii/sdk/packages/go/iii` | [`packages/go/iii`](packages/go/iii/README.md) |

Each package README has the full API, options, and examples.

## The API

| Operation | TypeScript |
| --- | --- |
| Connect to the engine | `const worker = registerWorker('ws://localhost:49134')` |
| Register a function | `worker.registerFunction('orders::create', handler)` |
| Call a function and wait for the result | `await worker.call({ function_id: 'orders::create', payload })` |
| Call a function and do not wait | `worker.call({ function_id: 'orders::create', payload, action: CallAction.Void() })` |
| Run function `b` when function `a` runs | `worker.observe('orders::create', 'alerts::big-order')` |

Function IDs use `::` as the separator, for example `orders::create`.

## Example

```typescript
import { registerWorker } from 'iii-sdk'

const worker = registerWorker('ws://localhost:49134')

worker.registerFunction('orders::create', async (order: { item: string; total: number }) => {
  return { id: crypto.randomUUID(), ...order }
})

worker.registerFunction('alerts::big-order', async ({ input }: { input: { total: number } }) => {
  console.log(`Big order: ${input.total}`)
})

// When orders::create runs with a total of 500 or more, alerts::big-order runs
worker.observe(
  { function_id: 'orders::create', filters: { 'input.total': { $gte: 500 } } },
  'alerts::big-order',
)

const order = await worker.call({ function_id: 'orders::create', payload: { item: 'widget', total: 750 } })
console.log(order)
```

```python
from iii import register_worker

worker = register_worker("ws://localhost:49134")


def create_order(order):
    return {"id": "order-1", **order}


def big_order(observed):
    print(f"Big order: {observed['input']['total']}")


worker.register_function("orders::create", create_order)
worker.register_function("alerts::big-order", big_order)

# When orders::create runs with a total of 500 or more, alerts::big-order runs
worker.observe(
    {"function_id": "orders::create", "filters": {"input.total": {"$gte": 500}}},
    "alerts::big-order",
)

order = worker.call({"function_id": "orders::create", "payload": {"item": "widget", "total": 750}})
print(order)
```

## Other packages

| Path | Contents |
| --- | --- |
| `packages/*/helpers` | Shared helpers for each language |
| `packages/*/observability` | OpenTelemetry integration for each language |
| `packages/*/iii-example` | Example workers |
| [`fixtures/`](fixtures/README.md) | Cross-SDK protocol and filter fixtures |

## Development

Run from the repository root:

```bash
pnpm test:sdk-node                                         # TypeScript
cargo test -p iii-sdk                                      # Rust
cd sdk/packages/python/iii && uv sync --extra dev && uv run pytest   # Python
cd sdk/packages/go/iii && go test ./...                    # Go
```

See [CONTRIBUTING.md](../CONTRIBUTING.md) for the full setup.

## License

[Apache License 2.0](LICENSE)
