# iii-bridge

Connects this III Engine instance to another III instance over iii-sdk so you can expose local functions to the remote instance and forward local calls to remote functions.

## Sample Configuration

```yaml
- name: iii-bridge
  config:
    url: ${REMOTE_III_URL:ws://0.0.0.0:49134}
    service_id: bridge-client
    service_name: bridge-client
    expose:
      - local_function: engine::log::info
        remote_function: engine::log::info

    forward:
      - local_function: remote::state::get
        remote_function: state::get
        timeout_ms: 5000

      - local_function: remote::state::set
        remote_function: state::set
        timeout_ms: 5000

      - local_function: remote::state::delete
        remote_function: state::delete
        timeout_ms: 5000
```

## Configuration

| Field | Type | Description |
|---|---|---|
| `url` | string | WebSocket URL of the remote III instance. Defaults to `III_URL` env var, otherwise `ws://0.0.0.0:49134`. |
| `service_id` | string | Service identifier to register with the remote instance. |
| `service_name` | string | Human-readable service name. Defaults to `service_id`. |
| `expose` | ExposeFunctionConfig[] | Functions from this instance to expose to the remote instance. |
| `forward` | ForwardFunctionConfig[] | Local function aliases that forward invocations to remote functions. |

### ExposeFunctionConfig

| Field | Type | Description |
|---|---|---|
| `local_function` | string | Required. Local function the remote instance will call. |
| `remote_function` | string | Function path to register on the remote instance. Defaults to `local_function`. |

### ForwardFunctionConfig

| Field | Type | Description |
|---|---|---|
| `local_function` | string | Required. Function path to register locally. |
| `remote_function` | string | Required. Remote function path to invoke when the local alias is called. |
| `timeout_ms` | number | Override the invocation timeout in milliseconds. Defaults to `30000`. |

## Functions

### `bridge.invoke`

Invoke a function on the remote III instance and wait for its response.

| Field | Type | Description |
|---|---|---|
| `function_id` | string | Required. Remote function ID to invoke. |
| `data` | any | Payload to send to the remote function. |
| `timeout_ms` | number | Override the invocation timeout in milliseconds. |

Returns the remote function's response value directly.

### `bridge.invoke_async`

Fire-and-forget invoke on the remote III instance.

| Field | Type | Description |
|---|---|---|
| `function_id` | string | Required. Remote function ID to invoke. |
| `data` | any | Payload to send to the remote function. |
