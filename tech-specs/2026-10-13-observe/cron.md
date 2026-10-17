# Worked example: `cron::on-tick`

Source: `workers/cron` at `079611a1f` (workers `origin/main`). Delivery:
**owner** (`obs.emit`): cron routes each tick to the observers of the
expression that fired.

## 1. Today

- `boot::start` (`src/boot.rs`) registers the trigger type `cron`:
  `RegisterTriggerType::new("cron", "Cron-based scheduled triggers", handler)`
  with `trigger_request_format::<CronTriggerSpec>()`. A guard refuses to start
  while the built-in `iii-cron` worker is connected.
- The trigger config (`src/trigger.rs` `CronTriggerSpec`):
  `{ expression, condition_function_id? }`.
- `CronTriggerHandler::register_trigger` parses it and calls
  `Scheduler::register(JobSpec { trigger_id, expression, function_id, condition_function_id, metadata })`.
- `Scheduler` (`src/scheduler.rs`) spawns **one tokio task per binding**. Each
  task sleeps until the next UTC fire (no catch-up), then:
  1. `lock.try_acquire(trigger_id)` (`src/locks.rs`: `local`, or `redis` with
     `SET NX PX`, 30 s TTL);
  2. builds `{ "trigger": "cron", "job_id": trigger_id, "scheduled_time", "actual_time" }`;
  3. calls `condition_function_id` if set (only an explicit `false` skips);
  4. `iii.trigger(function_id, payload)` with the binding's metadata as the
     sidecar (the harness resolves reactions from it);
  5. releases the lock.
- Ten bindings on `0 0 9 * * Mon-Fri` are ten tasks, ten lock round trips and
  ten fires that compute the same instant.
- Changing the lock adapter hot-swaps the scheduler and re-registers every
  `JobSpec`.

## 2. Target

### 2.1 The function

```rust
pub const ON_TICK_FN: &str = "cron::on-tick";

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CronTick {
    pub expression: String,          // the expression that fired, exactly as observers passed it in args
    pub scheduled_time: String,      // RFC 3339, UTC
    pub actual_time: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CronArgs {
    pub expression: String,          // UTC, 6 or 7 fields
}

let on_tick = iii.register_function(
    ON_TICK_FN,
    RegisterFunction::new({ let schedules = schedules.clone();
                            move |tick: CronTick| schedules.dispatch(tick) })   // §2.3
        .description("Fires on a cron schedule. Observe it with args { expression } \
                      (UTC, 6 or 7 fields).")
        // request_format / response_format come from the handler's argument and return types
        // (JsonSchema); .request_format(json!({ … })) overrides them with a Value
        .observe(Observe::owner()                                         // cron picks who gets each tick
            .args_format::<CronArgs>()                                    // engine: INVALID_ARGS
            .legacy_trigger_type("cron")
            .handler(schedules.clone())),
);
```

`args_format` replaces `trigger_request_format::<CronTriggerSpec>()`: a missing
`expression` or an unknown argument is rejected by the engine with
`INVALID_ARGS` before the worker hears about the observer.

- **Owner delivery.** The tick is not offered to every observer of
  `cron::on-tick`. The function's handler hands it to the observers of the
  expression that fired, with `obs.emit(tick)`. The observers' filters are then
  applied by the SDK, and the target receives the `Observed` envelope.
- `job_id` is gone: one tick now serves every observer of that expression.
  A target that needs its observer's id reads `ctx.activation.observer_id`.
- During phase 5 the tick also carries `"trigger": "cron"`, so legacy handlers
  that switch on it keep working.

### 2.2 Schedules from observers

```rust
#[async_trait]
impl ObserveHandler for Schedules {
    async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection> {
        let expression = match obs.legacy() {
            Some(l) => legacy_expression(&l.config)?,                    // §2.4
            None => obs.args::<CronArgs>()?.expression,                  // validated by the engine
        };
        let scheduler = self.scheduler.read().await.clone();
        scheduler.retain(&expression, obs.clone()).await                 // parses; one job per distinct expression
            .map_err(|err| ObserveRejection::new("INVALID_CRON_EXPRESSION", err.to_string()))
    }

    async fn on_observer_removed(&self, obs: &Observer) {
        self.scheduler.read().await.release(obs.id()).await;              // job stops when its last observer leaves
    }
}
```

`Scheduler` changes from `trigger_id → JobEntry` to:

```rust
jobs:   HashMap<String /*expression*/, JobEntry { handle, observers: HashMap<ObsId, Observer> }>,
by_obs: HashMap<ObsId, String /*expression*/>,
```

- `retain` parses the expression (same `Schedule::from_str`), spawns the job
  only if the expression is new, and adds the observer to its entry.
- `release` removes the observer from its expression's entry and aborts the
  job when the entry becomes empty.
- `job_specs()` (adapter hot-swap) returns expressions, not bindings.
- The observer is in its expression's entry before `on_observer_added` returns, so
  before the ack: the first fire after the engine marks it `active` reaches it.

### 2.3 Firing

```rust
// the job, one per expression
loop {
    sleep_until_next(&schedule).await;
    if !lock.try_acquire(&expression).await { continue }        // one lock op per expression, not per binding
    let tick = CronTick { expression: expression.clone(), scheduled_time, actual_time };
    // local dispatch: runs the handler below in-process, 0 frames
    let _ = iii.trigger(TriggerRequest::new(ON_TICK_FN, json!(tick))).await;
    lock.release(&expression).await;
}

// the function's handler: hand the tick to that expression's observers
impl Schedules {
    fn dispatch(&self, tick: CronTick) -> Result<(), Error> {
        for obs in self.observers(&tick.expression) {
            obs.emit(&tick)?;     // the SDK checks this observer's filters; a match joins the batched report.
                                  // Ok(false) on a miss or on an observer removed meanwhile: no guard needed
        }
        Ok(())
    }
}
```

- **One expression per observer.** To fire on two schedules, observe
  twice. Observers that share an expression share its job, and the handler
  only emits to the observers of the expression that fired.
- An observer can add filters, which the SDK evaluates on each emit:
  `{ "input.scheduled_time": { "$prefix": "2026-10-13" } }`. A miss sends
  nothing.
- Emits of one tick in one flush are coalesced into one report item with
  several `matched` ids; the engine activates each target with its own
  metadata.
- **The handler is callable.** `trigger('cron::on-tick', { expression,
  scheduled_time, actual_time })` fires that expression's observers now,
  which tests use. RBAC must not expose it to untrusted sessions.
- `condition_function_id` is not carried over (README §11.5): use a filter, or
  check inside the target.
- Metadata: each activation gets its observer's metadata as the sidecar, so
  harness reactions (`harness::spawn`, `harness::notify_agent`) keep resolving
  which binding fired.

### 2.4 Legacy callers

The function declares `legacy_trigger_type: "cron"`. An old
`registerTrigger({ type: 'cron', function_id, config: { expression } })`
becomes an observer with `obs.legacy.config` set
([`protocol.md`](protocol.md) §6). `legacy_expression` validates the config as
`CronTriggerHandler` does ("expression required") and returns the expression,
so the legacy observer joins that expression's job like a new one. The
target receives the tick (`deliver: "input"`) with the trigger metadata as the
sidecar.

A legacy config with `condition_function_id` is rejected with
`CRON_CONDITION_UNSUPPORTED` and a migration hint. The harness already refuses
conditions in its own bindings (`harness/src/functions/subscribe.rs`). If usage
data shows conditions in the wild, the worker instead keeps the old `cron`
trigger type registered beside the function until phase 6.

### 2.5 Observer examples

```ts
// weekday digest
iii.observe({
  function_id: 'cron::on-tick',
  args: { expression: '0 0 9 * * Mon-Fri' },
  deliver: 'input',                     // the target receives the CronTick itself
}, 'reports::daily')

// same expression, another worker: shares the job
iii.registerFunction('billing::send-reminders', ({ input }: Observed<CronTick>) =>
  billing.sendReminders(input.scheduled_time))

iii.observe({
  function_id: 'cron::on-tick',
  args: { expression: '0 0 9 * * Mon-Fri' },
  filters: { 'input.scheduled_time': { $gte: '2026-11-01T00:00:00Z' } },   // start in November
}, 'billing::send-reminders')

// one-shot wake-up
iii.observe({
  function_id: 'cron::on-tick',
  args: { expression: '0 30 14 13 10 * 2026' },
}, 'harness::notify_agent', {
  metadata: { session_id: 's1', task: 'check the deploy' },
  max_fires: 1,
})

// rejected by the engine: INVALID_ARGS (expression is required)
iii.observe({ function_id: 'cron::on-tick' }, 'reports::daily')
```

### 2.6 End to end

The same schedule, from both sides, in order:

```ts
// reports worker
iii.registerFunction('reports::daily-digest', ({ input }) => sendDigest(input.scheduled_time))

const digest = iii.observe({
  function_id: 'cron::on-tick',
  args: { expression: '0 0 9 * * Mon-Fri' },                              // → cron's onObserverAdded
  filters: { 'input.scheduled_time': { $gte: '2026-11-01T00:00:00Z' } },  // → checked on every emit
}, 'reports::daily-digest')
await digest.ready()
// later
await digest.dispose()
```

1. The cron worker registers `cron::on-tick` with its `observe` descriptor; the
   engine answers `functionregistered`.
2. The reports worker registers its target, `reports::daily-digest`.
3. `observe`: the engine checks `args` against `args_format` (`INVALID_ARGS`),
   the filters (`INVALID_FILTER`) and RBAC, and stores the observer as
   `pending`.
4. The engine sends `observeradded` to cron. `onObserverAdded` starts the job for
   `0 0 9 * * Mon-Fri`, or joins the running one, and returns; the SDK sends
   `observerack`.
5. The engine marks the observer `active` and sends `observeresult`;
   `ready()` resolves.
6. At 09:00 on a weekday the job calls `cron::on-tick` in-process; the handler
   calls `obs.emit(tick)`; the SDK checks `input.scheduled_time >= 2026-11-01`.
   Before November `emit` returns `false` and nothing is sent. From November
   the event joins the batched `invocationsobserved` report, and the engine
   sends `invokefunction(reports::daily-digest, envelope)`.
7. `dispose()` sends `unobserve`; the engine sends `observerremoved` to
   cron; `onObserverRemoved` removes the observer and stops the job if it was the
   expression's last.

## 3. Cost per fire

| Bindings / observers on one expression | Today | New |
|---|---|---|
| 0 | no job | no job |
| 1 | 1 task, 1 lock op, 2 frames (cron → engine → target) | 1 task, 1 lock op, 2 frames (1 report + 1 activation) |
| 3 | 3 tasks, 3 lock ops, 6 frames | 1 task, 1 lock op, 4 frames (1 report + 3 activations) |
| 10 | 10 tasks, 10 lock ops, 20 frames | 1 task, 1 lock op, 11 frames |
| 10 on 10 different expressions | 10 tasks, 20 frames | 10 tasks, 20 frames |

With the `redis` adapter a lock op is a network round trip, so sharing a job
also removes Redis traffic.

## 4. Acceptance

- The existing cron test suites pass with bindings registered through
  `registerTrigger` (shim) and through `observe`.
- New tests:
  - `INVALID_ARGS` for missing `expression`, for a non-string one and for unknown args;
  - `INVALID_CRON_EXPRESSION` with the parser's message;
  - two observers on one expression share one job; disposing one keeps the job, disposing both stops it;
  - two observers on different expressions only receive their own ticks (the handler emits per expression);
  - a filter miss on `emit` sends nothing; a direct `trigger('cron::on-tick', tick)` fires that expression's observers;
  - metadata of each observer arrives as its activation's sidecar;
  - legacy binding: same payload fields (minus `job_id`), same metadata, rejected when it carries `condition_function_id`;
  - adapter hot-swap re-creates one job per expression and never runs a fire twice.
- Benchmark: 1 000 observers over 50 expressions, every second: worker CPU,
  lock ops/s on Redis, frames/s, today vs new.
