//! The legacy shim (protocol.md §6), over real WebSockets: an old
//! `registertrigger` for a type that no live provider serves, but a function
//! stands in for (`observe.legacy_trigger_type`), becomes an observer of that
//! function, and the old consumer cannot tell the difference.

mod observe_support;

use iii::observer::ObserversConfig;
use iii::protocol::ObserverState;
use observe_support::{
    Client, Setup, TestEngine, connect, eventually, in_process, registered, spawn, spawn_engine,
};
use serde_json::{Value, json};
use uuid::Uuid;

/// A cron-like owner: `cron::on-tick` stands in for the `cron` trigger type.
/// It speaks `observe.v1` and acks by hand.
async fn cron_owner(t: &TestEngine, name: &str) -> Client {
    let mut cron = connect(t, name, true).await;
    cron.manual_acks();
    cron.register_function_with(json!({
        "type": "registerfunction",
        "id": "cron::on-tick",
        "observe": {
            "delivery": "owner",
            "legacy_trigger_type": "cron",
            "args_format": {
                "type": "object",
                "required": ["expression"],
                "properties": { "expression": { "type": "string" } }
            }
        }
    }))
    .await;
    cron.expect("functionregistered", |f| {
        f["type"] == "functionregistered" && f["id"] == "cron::on-tick"
    })
    .await;
    cron
}

/// A pre-0.25 worker hosting `billing::remind`.
async fn legacy_consumer(t: &TestEngine) -> Client {
    let mut billing = connect(t, "billing", false).await;
    billing.register_function("billing::remind").await;
    registered(&t.engine, "default", "billing::remind").await;
    billing
}

fn register_trigger(id: &str, expression: &str) -> Value {
    json!({
        "type": "registertrigger",
        "id": id,
        "trigger_type": "cron",
        "function_id": "billing::remind",
        "config": { "expression": expression },
        "metadata": { "__binding": format!("binding-{id}") }
    })
}

async fn added(client: &mut Client, id: &str) -> Value {
    let id = id.to_string();
    client
        .expect("observeradded", move |f| {
            f["type"] == "observeradded" && f["observer"]["id"] == id.as_str()
        })
        .await
}

async fn removed(client: &mut Client, id: &str) -> Value {
    let id = id.to_string();
    client
        .expect("observerremoved", move |f| {
            f["type"] == "observerremoved" && f["id"] == id.as_str()
        })
        .await
}

async fn ack(client: &mut Client, id: &str, revision: &Value, error: Option<&str>) {
    let mut frame = json!({
        "type": "observerack",
        "function_id": "cron::on-tick",
        "id": id,
        "revision": revision,
    });
    if let Some(code) = error {
        frame["error"] = json!({ "code": code, "message": "rejected by the cron owner" });
    }
    client.send(frame).await;
}

async fn result(client: &mut Client, id: &str) -> Value {
    let id = id.to_string();
    client
        .expect("triggerregistrationresult", move |f| {
            f["type"] == "triggerregistrationresult" && f["id"] == id.as_str()
        })
        .await
}

/// Once this returns, every notification queued for `cron` and `billing`
/// before it has arrived: a registration whose `observeradded` (to cron) and
/// result (to billing) go through the same per-connection queues, after.
async fn barrier(cron: &mut Client, billing: &mut Client) {
    let id = format!("barrier-{}", Uuid::new_v4());
    billing.send(register_trigger(&id, "0 * * * * *")).await;
    let added = added(cron, &id).await;
    ack(cron, &id, &added["revision"], None).await;
    result(billing, &id).await;
}

/// The next owner-sync frame `cron` got, in arrival order.
async fn next_sync(cron: &mut Client) -> Value {
    cron.expect("owner-sync frame", |f| {
        matches!(
            f["type"].as_str(),
            Some("functionregistered" | "observeradded" | "observerremoved")
        )
    })
    .await
}

fn state(t: &TestEngine, id: &str) -> Option<ObserverState> {
    t.engine.observers.get(id).map(|o| o.state())
}

/// `cron::on-tick` fires the job: an `obs.emit` report naming `matched`.
async fn tick(cron: &mut Client, matched: &[&str], scheduled_time: &str) {
    cron.send(json!({
        "type": "invocationsobserved",
        "items": [{
            "function_id": "cron::on-tick",
            "invocation_id": Uuid::new_v4().to_string(),
            "matched": matched,
            "status": "success",
            "input": { "scheduled_time": scheduled_time },
            "started_at_ms": 1_760_350_000_000u64,
            "duration_ms": 0.1
        }]
    }))
    .await;
}

#[tokio::test]
async fn an_old_trigger_becomes_an_observer_of_the_function_standing_in_for_its_type() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut cron = cron_owner(&t, "cron").await;
    let mut billing = legacy_consumer(&t).await;

    billing
        .send(register_trigger("t-remind", "0 0 9 * * Mon-Fri"))
        .await;
    let added = added(&mut cron, "t-remind").await;
    assert_eq!(added["function_id"], "cron::on-tick");
    assert_eq!(
        added["observer"],
        json!({
            "id": "t-remind",
            "args": {},
            "filters": {},
            "target": "billing::remind",
            "capture": ["input", "output", "caller"],
            "on": "success",
            "legacy": { "trigger_type": "cron", "config": { "expression": "0 0 9 * * Mon-Fri" } }
        }),
        "the owner reads the old config as the observer's args"
    );
    ack(&mut cron, "t-remind", &added["revision"], None).await;
    let answer = result(&mut billing, "t-remind").await;
    assert_eq!(answer["trigger_type"], "cron");
    assert_eq!(answer["function_id"], "billing::remind");
    assert!(answer.get("error").is_none(), "{answer}");
    assert!(eventually(|| state(&t, "t-remind") == Some(ObserverState::Active)).await);

    tick(&mut cron, &["t-remind"], "2026-10-16T09:00:00Z").await;
    let fire = billing.invoked("billing::remind").await;
    assert_eq!(
        fire["data"],
        json!({ "scheduled_time": "2026-10-16T09:00:00Z" }),
        "deliver: input — what the trigger handler received"
    );
    assert_eq!(fire["metadata"], json!({ "__binding": "binding-t-remind" }));
    for key in ["caller", "observe_depth", "activation"] {
        assert!(fire.get(key).is_none(), "`{key}` sent to an old worker");
    }
    billing.reply(&fire, Value::Null).await;

    let info = t
        .engine
        .observers
        .get_info(&t.engine, None, "t-remind")
        .unwrap();
    assert_eq!(info.legacy.as_ref().unwrap().trigger_type, "cron");
    barrier(&mut cron, &mut billing).await;
    assert!(
        !billing.has_seen(|f| f["type"] == "observeresult" || f["type"] == "observerstatechanged"),
        "the registrant asked for a trigger: it hears about nothing else"
    );
}

#[tokio::test]
async fn the_owners_rejection_is_the_registration_result() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut cron = cron_owner(&t, "cron").await;
    let mut billing = legacy_consumer(&t).await;

    billing
        .send(register_trigger("t-bad", "every tuesday"))
        .await;
    let added_frame = added(&mut cron, "t-bad").await;
    ack(
        &mut cron,
        "t-bad",
        &added_frame["revision"],
        Some("INVALID_CRON_EXPRESSION"),
    )
    .await;
    let answer = result(&mut billing, "t-bad").await;
    assert_eq!(
        answer["error"]["code"], "INVALID_CRON_EXPRESSION",
        "{answer}"
    );
    assert_eq!(state(&t, "t-bad"), Some(ObserverState::Error));

    // Never added, so never removed: the next add is the next revision.
    billing
        .send(register_trigger("t-good", "0 * * * * *"))
        .await;
    let next = added(&mut cron, "t-good").await;
    assert_eq!(
        next["revision"].as_u64(),
        Some(added_frame["revision"].as_u64().unwrap() + 1)
    );
    assert!(!cron.has_seen(|f| f["type"] == "observerremoved"));
}

#[tokio::test]
async fn unregistering_the_trigger_removes_the_observer() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut cron = cron_owner(&t, "cron").await;
    let mut billing = legacy_consumer(&t).await;
    for id in ["t-a", "t-b"] {
        billing.send(register_trigger(id, "0 * * * * *")).await;
        let added = added(&mut cron, id).await;
        ack(&mut cron, id, &added["revision"], None).await;
        result(&mut billing, id).await;
    }

    billing
        .send(json!({ "type": "unregistertrigger", "id": "t-a", "trigger_type": "cron" }))
        .await;
    removed(&mut cron, "t-a").await;
    assert!(t.engine.observers.get("t-a").is_none());

    // The builtin works on them too.
    let mut admin = connect(&t, "admin", true).await;
    let call = admin
        .call("engine::unregister_trigger", json!({ "id": "t-b" }))
        .await;
    assert_eq!(admin.result(call).await["result"]["removed"], true);
    removed(&mut cron, "t-b").await;

    // Registering a removed id again is an ordinary registration.
    billing.send(register_trigger("t-a", "0 * * * * *")).await;
    let again = added(&mut cron, "t-a").await;
    ack(&mut cron, "t-a", &again["revision"], None).await;
    assert!(result(&mut billing, "t-a").await.get("error").is_none());
}

#[tokio::test]
async fn a_live_old_style_provider_wins() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut cron = cron_owner(&t, "cron").await;
    let mut billing = legacy_consumer(&t).await;

    // Taken by the shim while nothing else provides `cron`.
    billing
        .send(register_trigger("t-shim", "0 * * * * *"))
        .await;
    let added = added(&mut cron, "t-shim").await;
    ack(&mut cron, "t-shim", &added["revision"], None).await;
    result(&mut billing, "t-shim").await;

    // An old-style provider of `cron` registers: new registrations go to it.
    let mut provider = connect(&t, "old-cron", false).await;
    provider
        .send(json!({ "type": "registertriggertype", "id": "cron", "description": "old cron" }))
        .await;
    assert!(
        eventually(|| t
            .engine
            .trigger_registry
            .trigger_types
            .contains_key(&iii::trigger::type_key("default", "cron")))
        .await
    );
    billing.send(register_trigger("t-old", "0 * * * * *")).await;
    let forwarded = provider
        .expect("registertrigger", |f| {
            f["type"] == "registertrigger" && f["id"] == "t-old"
        })
        .await;
    assert_eq!(forwarded["config"], json!({ "expression": "0 * * * * *" }));
    assert!(t.engine.observers.get("t-old").is_none());
    assert!(t.engine.trigger_registry.triggers.contains_key("t-old"));

    // What the shim took stays with it.
    assert_eq!(state(&t, "t-shim"), Some(ObserverState::Active));
    assert!(!provider.has_seen(|f| f["type"] == "registertrigger" && f["id"] == "t-shim"));
}

#[tokio::test]
async fn a_parked_trigger_is_taken_when_its_stand_in_registers() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut billing = legacy_consumer(&t).await;
    billing
        .send(register_trigger("t-early", "0 * * * * *"))
        .await;
    assert!(
        eventually(|| t
            .engine
            .trigger_registry
            .pending_triggers
            .contains_key("t-early"))
        .await,
        "nothing provides `cron` yet: parked"
    );

    let mut cron = cron_owner(&t, "cron").await;
    let added = added(&mut cron, "t-early").await;
    assert_eq!(added["observer"]["legacy"]["trigger_type"], "cron");
    ack(&mut cron, "t-early", &added["revision"], None).await;
    assert!(result(&mut billing, "t-early").await.get("error").is_none());
    assert!(
        !t.engine
            .trigger_registry
            .pending_triggers
            .contains_key("t-early")
    );
}

#[tokio::test]
async fn a_legacy_observer_waits_for_its_owner_like_any_observer() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut cron = cron_owner(&t, "cron").await;
    let mut billing = legacy_consumer(&t).await;
    billing
        .send(register_trigger("t-kept", "0 * * * * *"))
        .await;
    let added = added(&mut cron, "t-kept").await;
    ack(&mut cron, "t-kept", &added["revision"], None).await;
    result(&mut billing, "t-kept").await;

    cron.disconnect().await;
    assert!(eventually(|| state(&t, "t-kept") == Some(ObserverState::Pending)).await);

    let mut cron = connect(&t, "cron-2", true).await;
    cron.manual_acks();
    cron.register_function_with(json!({
        "type": "registerfunction",
        "id": "cron::on-tick",
        "observe": { "delivery": "owner", "legacy_trigger_type": "cron" }
    }))
    .await;
    let set = cron
        .expect("functionregistered", |f| {
            f["type"] == "functionregistered" && f["id"] == "cron::on-tick"
        })
        .await;
    assert_eq!(set["observers"][0]["id"], "t-kept");
    assert_eq!(
        set["observers"][0]["legacy"]["config"]["expression"],
        "0 * * * * *"
    );
    ack(&mut cron, "t-kept", &set["revision"], None).await;
    assert!(eventually(|| state(&t, "t-kept") == Some(ObserverState::Active)).await);

    tick(&mut cron, &["t-kept"], "2026-10-16T10:00:00Z").await;
    let fire = billing.invoked("billing::remind").await;
    billing.reply(&fire, Value::Null).await;
    // One registration, one result: the owner coming back is not news to it.
    barrier(&mut cron, &mut billing).await;
    assert!(!billing.has_seen(|f| f["type"] == "triggerregistrationresult"));
}

#[tokio::test]
async fn registering_the_same_trigger_again_replays_or_replaces_it() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut cron = cron_owner(&t, "cron").await;
    let mut billing = legacy_consumer(&t).await;
    billing
        .send(register_trigger("t-same", "0 * * * * *"))
        .await;
    let first = added(&mut cron, "t-same").await;
    ack(&mut cron, "t-same", &first["revision"], None).await;
    result(&mut billing, "t-same").await;

    // The same registration again (a reconnecting SDK): answered, nothing new.
    billing
        .send(register_trigger("t-same", "0 * * * * *"))
        .await;
    assert!(result(&mut billing, "t-same").await.get("error").is_none());

    // Another config under the same id replaces the binding: the next owner
    // messages are its removal and its new add (the replay sent none).
    billing
        .send(register_trigger("t-same", "0 0 * * * *"))
        .await;
    let removal = next_sync(&mut cron).await;
    assert_eq!(
        (removal["type"].as_str(), removal["id"].as_str()),
        (Some("observerremoved"), Some("t-same"))
    );
    let replaced = next_sync(&mut cron).await;
    assert_eq!(replaced["type"], "observeradded");
    assert_eq!(
        replaced["observer"]["legacy"]["config"]["expression"],
        "0 0 * * * *"
    );
    ack(&mut cron, "t-same", &replaced["revision"], None).await;
    assert!(result(&mut billing, "t-same").await.get("error").is_none());
}

#[tokio::test]
async fn a_stand_in_without_observe_v1_is_ignored() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut old = connect(&t, "old-cron", false).await;
    old.register_function_with(json!({
        "type": "registerfunction",
        "id": "cron::on-tick",
        "observe": { "delivery": "owner", "legacy_trigger_type": "cron" }
    }))
    .await;
    registered(&t.engine, "default", "cron::on-tick").await;
    let mut billing = legacy_consumer(&t).await;
    billing
        .send(register_trigger("t-parked", "0 * * * * *"))
        .await;
    assert!(
        eventually(|| t
            .engine
            .trigger_registry
            .pending_triggers
            .contains_key("t-parked"))
        .await,
        "the shim needs an owner that acks: the binding parks"
    );
    assert!(t.engine.observers.get("t-parked").is_none());
}

/// The trigger registration hook applies to what the shim takes, as it
/// does to old triggers.
#[tokio::test]
async fn the_trigger_registration_hook_applies_to_shim_registrations() {
    let t = spawn(Setup {
        rbac: Some(json!({
            "on_trigger_registration_function_id": "test::trigger-hook",
            "expose_functions": ["match(\"billing::*\")", "match(\"cron::*\")"]
        })),
        ..Setup::default()
    })
    .await;
    in_process(
        &t.engine,
        "test::trigger-hook",
        |input| json!({ "config": { "expression": format!("hooked {}", input["config"]["expression"].as_str().unwrap_or_default()) } }),
    );
    let mut cron = cron_owner(&t, "cron").await;
    let mut billing = legacy_consumer(&t).await;
    billing
        .send(register_trigger("t-hooked", "0 * * * * *"))
        .await;
    let added = added(&mut cron, "t-hooked").await;
    assert_eq!(
        added["observer"]["legacy"]["config"]["expression"],
        "hooked 0 * * * * *"
    );
}

/// Trigger ids are global, as they were: another connection registering the
/// same id replaces the binding.
#[tokio::test]
async fn a_trigger_id_registered_by_another_connection_replaces_the_binding() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut cron = cron_owner(&t, "cron").await;
    let mut billing = legacy_consumer(&t).await;
    billing
        .send(register_trigger("t-shared", "0 * * * * *"))
        .await;
    let first = added(&mut cron, "t-shared").await;
    ack(&mut cron, "t-shared", &first["revision"], None).await;
    result(&mut billing, "t-shared").await;

    let mut audit = connect(&t, "audit", false).await;
    audit.register_function("audit::log").await;
    registered(&t.engine, "default", "audit::log").await;
    let mut frame = register_trigger("t-shared", "0 * * * * *");
    frame["function_id"] = json!("audit::log");
    audit.send(frame).await;
    let removal = next_sync(&mut cron).await;
    assert_eq!(removal["type"], "observerremoved");
    let second = next_sync(&mut cron).await;
    assert_eq!(second["observer"]["target"], "audit::log");
    ack(&mut cron, "t-shared", &second["revision"], None).await;
    assert!(result(&mut audit, "t-shared").await.get("error").is_none());
    assert_eq!(
        t.engine.observers.get("t-shared").unwrap().target.1,
        "audit::log"
    );
}

/// A re-registration the shim refuses leaves the working binding alone; and
/// shim bindings are not held to observer limits old triggers never had.
#[tokio::test]
async fn shim_bindings_keep_old_trigger_semantics() {
    let t = spawn_engine(ObserversConfig {
        max_per_connection: 1,
        max_per_function: 1,
        ..ObserversConfig::default()
    })
    .await;
    let mut cron = cron_owner(&t, "cron").await;
    let mut billing = legacy_consumer(&t).await;
    let long_id = format!("t-{}", "x".repeat(400));
    for id in ["t-one", long_id.as_str()] {
        billing.send(register_trigger(id, "0 * * * * *")).await;
        let added = added(&mut cron, id).await;
        ack(&mut cron, id, &added["revision"], None).await;
        assert!(
            result(&mut billing, id).await.get("error").is_none(),
            "{id}"
        );
    }

    // Its own stand-in as the target: refused, and `t-one` stays.
    let mut frame = register_trigger("t-one", "0 * * * * *");
    frame["function_id"] = json!("cron::on-tick");
    billing.send(frame).await;
    assert_eq!(
        result(&mut billing, "t-one").await["error"]["code"],
        "SELF_TARGET"
    );
    assert_eq!(state(&t, "t-one"), Some(ObserverState::Active));
}

/// Shim bindings show in the trigger builtins as old bindings did, listed
/// from the moment they are stored (also while the owner's ack is in
/// flight), marked with the function serving them; the legacy type stays
/// listed while a stand-in serves it.
#[tokio::test]
async fn shim_bindings_show_in_the_trigger_builtins() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut cron = cron_owner(&t, "cron").await;
    let mut billing = legacy_consumer(&t).await;
    let mut admin = connect(&t, "admin", true).await;
    async fn call(admin: &mut Client, function_id: &str, data: Value) -> Value {
        let id = admin.call(function_id, data).await;
        admin.result(id).await
    }

    billing
        .send(register_trigger("t-listed", "0 * * * * *"))
        .await;
    let added = added(&mut cron, "t-listed").await;
    // Not acked yet: already listed.
    let listed = call(&mut admin, "engine::registered-triggers::list", json!({})).await;
    let row = listed["result"]["registered_triggers"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["id"] == "t-listed")
        .cloned()
        .unwrap_or_else(|| panic!("not listed: {listed}"));
    assert_eq!(row["trigger_type"], "cron");
    assert_eq!(row["function_id"], "billing::remind");
    assert_eq!(row["status"], "active");
    assert_eq!(row["served_by"], "cron::on-tick");
    assert_eq!(row["observer_state"], "pending");
    assert_eq!(row["config"], json!({ "expression": "0 * * * * *" }));

    ack(&mut cron, "t-listed", &added["revision"], None).await;
    result(&mut billing, "t-listed").await;
    let info = call(
        &mut admin,
        "engine::registered-triggers::info",
        json!({ "id": "t-listed" }),
    )
    .await;
    let detail = &info["result"];
    assert_eq!(detail["trigger_type"], "cron", "{info}");
    assert_eq!(detail["observer_state"], "active");
    assert_eq!(detail["served_by"], "cron::on-tick");
    assert_eq!(
        detail["metadata"],
        json!({ "__binding": "binding-t-listed" })
    );
    assert_eq!(detail["trigger"]["id"], "cron");
    assert_eq!(detail["trigger"]["instance_count"], 1);
    assert_eq!(detail["function"]["function_id"], "billing::remind");

    let types = call(&mut admin, "engine::triggers::list", json!({})).await;
    assert!(
        types["result"]["triggers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == "cron" && t["worker_name"] == "cron"),
        "{types}"
    );
    let type_info = call(
        &mut admin,
        "engine::triggers::info",
        json!({ "id": "cron" }),
    )
    .await;
    assert_eq!(type_info["result"]["instance_count"], 1, "{type_info}");
}
