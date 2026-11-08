// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `emit`: owner delivery, the way cron or a database source works. One
//! worker hosts `bench::source` with `Observe::owner()` and keeps the
//! observers its hook is handed; emitter tasks hand every event to every
//! observer with `obs.emit()`, as fast as the SDK accepts them. Emits are
//! batched into reports, and the engine activates each observer's target.
//! What comes back: emits accepted per second, activations delivered, and
//! what the SDK or the engine dropped on the way.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use iii_sdk::{
    Error, Observe, ObserveHandler, ObserveRejection, Observer, RegisterFunction, Source,
};
use serde_json::{Value, json};

use super::{
    Env, Measured, drain, observer_totals, shutdown, sink, wait_callable, worker, workers,
};
use crate::stats::Load;

const SINKS: usize = 4;
const EMITTERS: usize = 4;
/// Observers the paced ladder emits to, round-robin.
const PACED_OBSERVERS: usize = 16;
/// Offered activations per second, all observers together.
const PACED_RATES: &[u64] = &[1_000, 5_000, 20_000, 50_000, 100_000, 200_000, 500_000];
/// A rate is sustained when at least this share of it is delivered.
const SUSTAINED: f64 = 0.95;

/// The observers the owner was handed.
#[derive(Clone, Default)]
struct Registry(Arc<Mutex<Vec<Observer>>>);

impl ObserveHandler for Registry {
    async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection> {
        self.0.lock().unwrap().push(obs.clone());
        Ok(())
    }

    async fn on_observer_removed(&self, obs: &Observer) {
        self.0.lock().unwrap().retain(|o| o != obs);
    }
}

pub async fn run(env: &Env) -> Result<()> {
    let registry = Registry::default();
    let owner = worker(&env.url, "owner").await?;
    owner.register_function(
        "bench::source",
        RegisterFunction::new(|event: Value| Ok::<_, Error>(event))
            .observe(Observe::owner().handler(registry.clone())),
    );
    let received = Arc::new(AtomicU64::new(0));
    let sinks = workers(&env.url, "sink", SINKS).await?;
    for (j, w) in sinks.iter().enumerate() {
        w.register_function(format!("bench::sink-{j}"), sink(received.clone()));
    }
    let observing = worker(&env.url, "observer").await?;
    for j in 0..SINKS {
        wait_callable(&observing, &format!("bench::sink-{j}")).await?;
    }
    let ctl = &observing;

    let mut handles = Vec::new();
    for k in env.levels(&[1, 16, 256], &[1, 16]) {
        for i in handles.len()..k {
            handles.push(observing.observe(
                Source::function("bench::source"),
                format!("bench::sink-{}", i % SINKS),
            ));
        }
        for h in &handles {
            env.guard(h.ready()).await??;
        }
        // The owner's hooks run when the engine asks; wait until it holds all k.
        let deadline = Instant::now() + Duration::from_secs(30);
        while registry.0.lock().unwrap().len() < k && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        drain(&received, Duration::from_secs(2)).await;

        let before = observer_totals(ctl).await?;
        let received0 = received.load(Ordering::Relaxed);
        let flood_registry = registry.clone();
        let flood_received = received.clone();
        env.step(
            format!("{k} observers"),
            json!({ "observers": k, "emitters": EMITTERS }),
            |deadline| async move {
                let rejected = Arc::new(AtomicU64::new(0));
                let mut tasks = Vec::new();
                for e in 0..EMITTERS {
                    let registry = flood_registry.clone();
                    let rejected = rejected.clone();
                    tasks.push(tokio::spawn(async move {
                        let mut load = Load::default();
                        let mut seq = 0u64;
                        while Instant::now() < deadline {
                            let observers = registry.0.lock().unwrap().clone();
                            for obs in &observers {
                                let t = Instant::now();
                                match obs.emit(&json!({ "n": seq, "emitter": e })) {
                                    Ok(true) => load.ok(t.elapsed()),
                                    Ok(false) => {
                                        rejected.fetch_add(1, Ordering::Relaxed);
                                    }
                                    Err(err) => load.err(err),
                                }
                            }
                            seq += 1;
                            // emit() never waits: give the connection its turn.
                            tokio::task::yield_now().await;
                        }
                        load
                    }));
                }
                let mut load = Load::default();
                for t in tasks {
                    load.merge(t.await?);
                }
                let ended = Instant::now();
                let secs = (ended - (deadline - env.opts.step)).as_secs_f64();
                let in_window = flood_received.load(Ordering::Relaxed) - received0;
                let delivered = drain(&flood_received, Duration::from_secs(10)).await - received0;
                let after = observer_totals(ctl).await?;
                Ok(Measured::timed(load, "emits").ended(ended).extra(json!({
                    "emits_not_accepted": rejected.load(Ordering::Relaxed),
                    "activations_per_sec": in_window as f64 / secs,
                    "activations_delivered": delivered,
                    "engine": after.since(&before),
                })))
            },
        )
        .await?;

        // The flood above shows what overload does; this finds what the
        // engine sustains: a paced ladder of offered rates over these 16
        // observers, up to the first that is not delivered in full.
        if k == PACED_OBSERVERS {
            for rate in env.levels(PACED_RATES, &[1_000, 20_000]) {
                let step = paced(env, &registry, &received, ctl, rate).await?;
                let ratio = step.extra["delivery_ratio"].as_f64().unwrap_or(0.0);
                if ratio < SUSTAINED {
                    break;
                }
            }
        }
    }

    // What the engine keeps once the load is over (peak vs. retained).
    drain(&received, Duration::from_secs(10)).await;
    env.idle("after the load (at rest)", json!({}), json!({}))
        .await?;

    for h in &handles {
        h.dispose().await.ok();
    }
    shutdown(sinks.into_iter().chain([owner, observing])).await;
    Ok(())
}

/// One paced step: `rate` emits a second, spread over the owner's observers
/// round-robin, for the step's length; then what arrived.
async fn paced(
    env: &Env,
    registry: &Registry,
    received: &Arc<AtomicU64>,
    ctl: &iii_sdk::IIIClient,
    rate: u64,
) -> Result<super::Step> {
    drain(received, Duration::from_secs(5)).await;
    let before = observer_totals(ctl).await?;
    let received0 = received.load(Ordering::Relaxed);
    let observers = registry.0.lock().unwrap().clone();
    env.step(
        format!(
            "{} observers, {} emits/s offered",
            observers.len(),
            crate::report::fmt_rate(rate as f64)
        ),
        json!({ "observers": observers.len(), "offered_per_sec": rate }),
        |deadline| async move {
            let mut tick = tokio::time::interval(Duration::from_millis(1));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
            let per_tick = rate as f64 / 1000.0;
            let (mut owed, mut offered, mut accepted, mut i) = (0.0, 0u64, 0u64, 0usize);
            while Instant::now() < deadline {
                tick.tick().await;
                owed += per_tick;
                while owed >= 1.0 {
                    owed -= 1.0;
                    offered += 1;
                    let obs = &observers[i % observers.len()];
                    i += 1;
                    if matches!(obs.emit(&json!({ "n": i })), Ok(true)) {
                        accepted += 1;
                    }
                }
            }
            let ended = Instant::now();
            let secs = (ended - (deadline - env.opts.step)).as_secs_f64();
            let in_window = received.load(Ordering::Relaxed) - received0;
            let delivered = drain(received, Duration::from_secs(10)).await - received0;
            let after = observer_totals(ctl).await?;
            Ok(Measured::counted(offered, "emits")
                .ended(ended)
                .extra(json!({
                    "offered_per_sec": rate,
                    "emits_accepted": accepted,
                    "activations_per_sec": in_window as f64 / secs,
                    "activations_delivered": delivered,
                    "activations_expected": offered,
                    "delivery_ratio": delivered as f64 / offered.max(1) as f64,
                    "engine": after.since(&before),
                })))
        },
    )
    .await
}
