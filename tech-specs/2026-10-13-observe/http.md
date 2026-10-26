# Worked example: `http::on-request`

Source: `workers/http` at `079611a1f` (workers `origin/main`). Delivery:
**owner**, because the HTTP response is the observer's result.

## 1. Today

- `boot::start` (`src/boot.rs`) registers the trigger type `http`:
  `RegisterTriggerType::new("http", "HTTP API trigger", handler)` with
  `call_request_format::<HttpRequest>()` and
  `trigger_request_format::<HttpTriggerConfig>()`. A guard refuses to start
  while the built-in `iii-http` worker is connected, because two owners of one
  trigger type collide (last write wins).
- The trigger config (`src/types.rs` `HttpTriggerConfig`):
  `{ api_path, http_method = "GET", public_webhook = false, condition_function_id?, middleware_function_ids = [] }`.
- `HttpTriggerHandler::register_trigger` (`src/trigger.rs`) turns each binding
  into a `Route` in the shared `RouteTable`, keyed by trigger id.
  `RouteTable::insert` rejects a route whose method + path *shape* collides with
  another binding's (`/a/:x` vs `/a/:y`). `unregister_trigger` removes by id;
  the SDK only fills `id` on unregister.
- `dynamic_handler` (`src/handler.rs`) serves each request:
  1. `match_route_for_listener(method, path, public_only)`; literal segments
     beat parameters; a path known under another method → 405 with `Allow`;
     nothing → 404;
  2. global `preHandler` middleware from the worker config;
  3. creates the request-body and response channels;
  4. builds `HttpRequest { query_params, path_params, headers, path, method,
     body, trigger: { type: "http", path, method }, request_body, response }`.
     Note: `path` is the matched **route template** (`route.http_path`), and
     `trigger.path` is the actual request path;
  5. `condition_function_id`, if set: a function call; `false` → 422
     `CONDITION_NOT_MET`;
  6. per-route `middleware_function_ids`, each a function call;
  7. `iii.trigger({ function_id: route.function_id, payload, timeout_ms })` in a
     spawned task, raced against the response channel (streamed vs buffered
     response).

## 2. Target

### 2.1 The function

```rust
pub const ON_REQUEST_FN: &str = "http::on-request";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HttpRouteArgs {
    pub method: String,                              // GET, POST, …
    pub path: String,                                // route template, e.g. /users/:id
    #[serde(default)] pub public_webhook: bool,
    #[serde(default)] pub middleware_function_ids: Vec<String>,
    pub condition_function_id: Option<String>,       // deprecated in favour of filters (README §11.5)
}

let routes = Arc::new(RwLock::new(RouteTable::default()));

iii.register_function(
    ON_REQUEST_FN,
    RegisterFunction::new_async({ let router = Router::new(routes.clone(), cell.clone());
                                  move |req: HttpRequest| router.clone().route(req) })
        .description("One invocation per HTTP request. Observe it with args { method, path } \
                      to serve that route; your target's return value is the response.")
        // request_format / response_format come from the handler's HttpRequest / HttpResponse types
        .observe(Observe::owner()
            .args_format::<HttpRouteArgs>()           // engine: INVALID_ARGS when method or path is missing
            .legacy_trigger_type("http")
            .handler(RouteObserver { routes: routes.clone() })),
);
```

`HttpRouteArgs` is today's `HttpTriggerConfig` with iii lang's names
(`request({ method, path })`): `http_method` → `method`, `api_path` → `path`.

`HttpRequest` gains one field, `route`: the matched template (`/users/:id`),
the same value `path` carries today. `path` and `trigger` keep their current
values during the deprecation window, so existing route handlers see the same
payload.

### 2.2 Routes from observers

```rust
#[async_trait]
impl ObserveHandler for RouteObserver {
    async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection> {
        let args = match obs.legacy() {
            Some(l) => from_legacy(&l.config)?,                         // §2.5
            None => obs.args::<HttpRouteArgs>()?,                       // method and path: checked by the engine
        };
        self.routes.write().await
            .insert(Route::new(obs.clone(), &args.method, &normalize_api_path(&args.path), &args))   // same conflict rule as today
            .map_err(|msg| ObserveRejection::new("HTTP_ROUTE_CONFLICT", msg))?;
        Ok(())                                                          // the route is how this owner routes by args
    }

    async fn on_observer_removed(&self, obs: &Observer) {
        self.routes.write().await.remove_by_observer_id(obs.id());
    }
}
```

- `RouteTable` is keyed by observer id instead of trigger id, and each `Route` holds its `Observer` (a cheap clone), so the router calls `matches` and `call` on it directly. Matching,
  precedence, 405 handling and the restricted listener (`public_webhook`) do not
  change.
- One route per observer. To serve a target on two methods or paths,
  observe twice.
- A conflict rejects the observer with today's message.

### 2.3 The router

The function's own handler is the router. It does what `dynamic_handler` does
between "route matched" and "response", minus the socket:

```rust
impl Router {
    async fn route(self, mut req: HttpRequest) -> Result<HttpResponse, Error> {
        let config = self.config.read().await.clone();
        let (route, params) = match req.route.as_deref() {
            Some(r) => self.routes.read().await.by_template(&req.method, r)?,     // the server already matched
            None    => self.routes.read().await.match_route(&req.method, &req.path)   // called by a worker
                         .ok_or(not_found())?,
        };
        req.route = Some(route.http_path.clone());
        req.path_params = params;
        let obs = &route.observer;                                             // the Observer kept in the route at insert
        let input = serde_json::to_value(&req)?;
        if !obs.matches(&Doc { input: Some(&input), ..Doc::default() }) {      // the observer's filters
            return Ok(condition_not_met());                                    // 422, as today
        }
        if let Some(cond) = &route.condition_function_id { /* as today */ }
        for mw in &route.middleware_function_ids { /* as today */ }
        obs.call::<HttpResponse>(&req).await                                   // InvokeFunction(target, activation)
    }
}
```

`obs.call` checks the observer's filters as well and rejects with
`FILTERS_NOT_MATCHED` without sending anything. The router checks
`obs.matches` first because a miss is a response it answers itself (422).

`dynamic_handler` keeps the socket work: matching for 404 / 405, global
middleware, channels, spans, the response race. Instead of
`iii.trigger(route.function_id)` it calls
`iii.trigger(TriggerRequest::new("http::on-request", payload))`, which runs the
router **in-process** (local dispatch, no frame). The response race works on
the returned future exactly as before.

What this buys besides the unified API:

- **Predicates without a condition function.** A route observed with
  `args: { method: 'POST', path: '/webhooks/stripe' }` and
  `filters: { 'input.headers.stripe-signature': { $exists: true } }` has its
  filters evaluated in-process by `obs.matches`. Today the same check is a
  `condition_function_id`: a full function call, four frames per request.
- **The router is a function.** Tests and internal callers can
  `trigger('http::on-request', { method, path, … })` and get the response
  without a socket. RBAC must not expose it to untrusted sessions.
- **Access logs without the http worker.** Every function is observable:
  observe the route's target (`users::get`) with `capture: ['input', 'output']`.
  Its invocations are routed through the engine, which evaluates the filters;
  the users worker and the http worker do nothing extra.

### 2.4 Observer examples

```ts
// a route; the target's return value is the response
iii.registerFunction('users::get', async (req: HttpRequest) =>
  ({ status_code: 200, body: await users.get(req.path_params.id) }))

iii.observe({
  function_id: 'http::on-request',
  args: { method: 'GET', path: '/users/:id' },
}, 'users::get')

// an existing function as the route target, with middleware
iii.observe({
  function_id: 'http::on-request',
  args: { method: 'POST', path: '/orders', middleware_function_ids: ['auth::verify-jwt'] },
}, 'orders::create')

// a public webhook that only accepts signed requests
iii.observe({
  function_id: 'http::on-request',
  args: { method: 'POST', path: '/webhooks/stripe', public_webhook: true },
  filters: { 'input.headers.stripe-signature': { $exists: true } },
}, 'billing::on-stripe-event')

// an access log on a route target (auto delivery; nothing to do with the http worker)
iii.registerFunction('ops::access-log', ({ input, output, duration_ms }: Observed<HttpRequest, HttpResponse>) =>
  accessLog.write({ input, status: output?.status_code, duration_ms }))

iii.observe({
  function_id: 'users::get',
  capture: ['input', 'output', 'caller'],
  on: 'complete',
}, 'ops::access-log')
```

### 2.5 Legacy callers

The function declares `legacy_trigger_type: "http"`. An old
`registerTrigger({ type: 'http', function_id, config })` becomes an
observer with `obs.legacy.config` = the `HttpTriggerConfig`
([`protocol.md`](protocol.md) §6). `from_legacy` parses it exactly as
`HttpTriggerHandler` does today (same defaults, same `api_path`
normalization) into `HttpRouteArgs`, so the route is inserted as for any
observer and keeps the old middleware, condition and listener settings.
`engine::observers::list` shows the legacy config. The target receives the
same `HttpRequest` as before.

## 3. Frames per request

| | Today (trigger) | New |
|---|---|---|
| Plain route | 4: http → engine → target, result back | 4: unchanged (`obs.call` is one routed invocation) |
| Route with `condition_function_id` | 8 | 4 when the condition is a filter predicate (evaluated in-process); 8 if the condition function is kept |
| Route with N middleware functions | 4 + 4N | 4 + 4N (middleware are still function calls) |
| 404 / 405 | 0 | 0 (answered by the server before invoking anything) |
| Access log on every request | a second route or middleware: +4 | an observer on the target: +1 activation frame engine → logger |
| Route registration | 4 (registertrigger, forward, result, result) | 4 (observe, observeradded, observerack, observeresult) |

HTTP does not get cheaper per request. The gains are one API instead of three,
engine-owned bindings (listable, RBAC-gated, cleaned up on disconnect),
declarative predicates in place of most condition functions, and observability
of route targets for free.

## 4. Acceptance

- The existing http test suites pass with routes registered through
  `registerTrigger` (shim) and through `observe`.
- New tests:
  - `INVALID_ARGS` when `method` or `path` is missing, or an unknown arg is passed;
  - `HTTP_ROUTE_CONFLICT` with today's message;
  - a filter on a header returns 422 when unmet, without any extra frame;
  - `obs.call` on a request that fails the filters rejects with `FILTERS_NOT_MATCHED` and sends no frame;
  - the restricted listener only serves observers with `args.public_webhook`;
  - `dispose()` on a route observer removes the route; losing the observing worker removes it after the grace period;
  - calling `http::on-request` from a worker returns the same response as the socket;
  - streamed responses (response channel) still win the race against `obs.call`.
- Benchmark: requests/s and p99 latency for a plain route and a route with a
  header predicate, today vs new, on the same hardware.
