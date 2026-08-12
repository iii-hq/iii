# Namespace inheritance in the SDKs

Two SDK defaults change: `trigger` and the low-level `register_trigger` stop
resolving in `default` and resolve in the worker's own namespace instead.

Not a compose change. This belongs to the namespace work (#1984), on a branch
of its own off `feat/namespare`.

## Why

A worker declares its namespace once, at registration. Everything else it does
should stay inside that namespace unless it says otherwise. Today two calls
leave it silently:

| call | target | today | after |
| --- | --- | --- | --- |
| `IIIClient::register_trigger` | the worker's own function | `default` | the worker's |
| `IIIClient::trigger` | another worker's function | `default` | the worker's |

The typed helper (`TriggerTypeRef::register_trigger`) already inherits, in all
three SDKs, each with a comment explaining the bug it fixes. The fix never
reached the low-level path, which is the one every worker in the fleet uses:
86 of 86 registrations in `iii-hq/workers` go through it, and none declares a
namespace.

## The audit

Everything in the SDK that names a function falls into two kinds.

**Follows the connection — already correct, nothing to do.**
`register_function`, `register_trigger_type`, `helpers::create_stream`. The
engine files these under the registering connection's namespace. There is no
field to set and no default to get wrong.

**Names a target — has to say where the target lives.**

| | status |
| --- | --- |
| `RegisterTriggerMessage.namespace` | field exists, default wrong — **changing** |
| `InvokeFunction.namespace` | field exists, default wrong — **changing** |
| `StreamJoinLeaveTriggerConfig.condition_function_id` | no field, and none needed |
| `OnTriggerRegistrationInput.namespace` | field exists, already correct |

`condition_function_id` needs nothing: `engine/src/condition.rs` evaluates it
with `call_with_metadata_ns(namespace, ...)`, where `namespace` is the
trigger's. It inherits the moment the trigger does.

`OnTriggerRegistrationInput` is an RBAC hook payload travelling *into* a hook
worker, not a call the SDK makes. It already carries the target namespace, with
the reason written on the field.

So the surface is exactly two calls.

## What breaks

`register_trigger` breaks nothing. All 86 registrations target the registering
worker's own function, so inheriting is what they already meant. Today any of
them running in a namespace has a trigger that registers, fires, and resolves
nothing.

`trigger` breaks callers of workers that live somewhere else. In
`iii-hq/workers`, counting literal `function_id` strings outside tests:

```
engine::            26 calls   always in default   -> breaks
configuration::      5 calls   always in default   -> breaks
state::             56 calls   depends on deploy
stream::             5 calls   depends on deploy
queue::              2 calls   depends on deploy
```

The split is not arbitrary. `configuration` and `engine::*` are compiled into
the engine and only ever exist in `default`. `state`, `queue`, `stream` are
packaged workers: the fleet may run one copy in `default`, or a compose project
may bring its own, in which case it registers in the project namespace and
inheriting is the correct resolution. A compose project that brings its own
`state` is exactly the case the old default gets wrong.

## Work

**1. Rust — `sdk/packages/rust/iii/src/iii.rs`**

`register_trigger`: when `input.namespace` is `None`, fill it from
`self.namespace()`.

`trigger`: when the request carries no namespace, send the worker's.

Both keep the explicit form. `default` becomes something a caller asks for by
name rather than something it gets by omission.

**2. Node — `sdk/packages/node/iii/src/iii.ts`**, same two entry points.

**3. Python — `sdk/packages/python/iii/src/iii/iii.py`**, same two.

**4. Tests, per SDK**

- a worker with a namespace registers a trigger without one: the message
  carries the worker's namespace;
- the same worker triggers without one: the invocation carries it;
- an explicit namespace still wins, including an explicit `default`;
- a worker with no namespace is unchanged.

**5. `architecture/SDK.md`** — state the rule once: a target with no namespace
resolves in the worker's own.

**6. One end-to-end scenario in `compose-smoke-tests`**

Unit tests only prove the field is filled. What proves the behaviour is a
worker in a namespace whose trigger actually fires. A container in `shop` with
a configuration trigger, a config change, and the handler running. That
scenario fails today against `iii-sdk 0.21.6`.

## Fallout to schedule, not to do here

The 31 `engine::` and `configuration::` calls in `iii-hq/workers` need an
explicit `default` before those workers can run in a namespace. They are
correct today by coincidence: their target's home matches the old default.
This lands with the SDK bump those workers already need — `RegisterTriggerInput`
grew a field they do not list, so they cannot compile against the new SDK
untouched anyway.

Worth deciding separately, and not here: `configuration::register` and
`state::set` are the same shape of string today while being different kinds of
thing. Until an engine builtin is distinguishable from a packaged worker, any
default is wrong for one of them.
