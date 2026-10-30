// Reproduction from https://github.com/iii-hq/iii/issues/2237.
// The four original scenarios are followed by related cancellation,
// replacement and provider-error regressions.

#[cfg(test)]
mod teardown_race_repro {
    use iii::protocol::DEFAULT_NAMESPACE;
    use iii::trigger::{
        RegisterTriggerOutcome, Trigger, TriggerRegistrator, TriggerRegistry, TriggerType,
    };
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
    use std::time::Duration;
    use std::{future::Future, pin::Pin};
    use tokio::sync::Notify;
    use uuid::Uuid;

    /// In-process provider whose next register / unregister can be paused
    /// once: the paused call signals `entered`, then waits for `resume`.
    #[derive(Default)]
    struct Gate {
        live: std::sync::Mutex<HashMap<String, Trigger>>,
        events: std::sync::Mutex<Vec<String>>,
        pause_register: AtomicBool,
        pause_unregister: AtomicBool,
        fail_register: AtomicBool,
        fail_unregister: AtomicBool,
        entered: Notify,
        resume: Notify,
    }

    impl Gate {
        fn pause(&self) -> impl Future<Output = ()> + '_ {
            let mut resumed = Box::pin(self.resume.notified());
            // Queue this waiter before publishing `entered`, so later calls
            // cannot consume its resume notification before its first poll.
            resumed.as_mut().enable();
            self.entered.notify_one();
            resumed
        }

        fn live(&self, id: &str) -> bool {
            self.live.lock().unwrap().contains_key(id)
        }
        fn live_ids(&self) -> Vec<String> {
            let mut ids: Vec<String> = self.live.lock().unwrap().keys().cloned().collect();
            ids.sort();
            ids
        }
        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }
        fn function(&self, id: &str) -> Option<String> {
            self.live
                .lock()
                .unwrap()
                .get(id)
                .map(|trigger| trigger.function_id.clone())
        }
    }

    struct GateRegistrator(Arc<Gate>);

    impl std::ops::Deref for GateRegistrator {
        type Target = Gate;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl TriggerRegistrator for GateRegistrator {
        fn register_trigger(
            &self,
            trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            Box::pin(async move {
                if self.pause_register.swap(false, SeqCst) {
                    self.pause().await;
                }
                if self.fail_register.swap(false, SeqCst) {
                    anyhow::bail!("register failed");
                }
                self.live
                    .lock()
                    .unwrap()
                    .insert(trigger.id.clone(), trigger.clone());
                self.events.lock().unwrap().push(format!("+{}", trigger.id));
                Ok(())
            })
        }

        fn unregister_trigger(
            &self,
            trigger: Trigger,
        ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
            Box::pin(async move {
                if self.pause_unregister.swap(false, SeqCst) {
                    self.pause().await;
                }
                if self.fail_unregister.swap(false, SeqCst) {
                    anyhow::bail!("unregister failed");
                }
                self.live.lock().unwrap().remove(&trigger.id);
                self.events.lock().unwrap().push(format!("-{}", trigger.id));
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn paused_calls_resume_in_announced_order() {
        let gate = Gate::default();
        let first = gate.pause();
        gate.entered.notified().await;
        let second = gate.pause();
        gate.entered.notified().await;
        tokio::pin!(first, second);

        // Neither waiter has been polled: a caller can be descheduled as soon
        // as it announces that it has paused.
        gate.resume.notify_one();
        assert!(
            futures::poll!(&mut second).is_pending(),
            "the second call consumed the first call's resume notification"
        );
        assert!(futures::poll!(&mut first).is_ready());

        gate.resume.notify_one();
        assert!(futures::poll!(&mut second).is_ready());
    }

    /// A fresh Box (fresh registrator identity) per announce, like router_msg.
    fn gate_type(ns: &str, type_id: &str, gate: &Arc<Gate>, owner: Option<Uuid>) -> TriggerType {
        TriggerType::new_ns(
            ns,
            type_id,
            "gate",
            Box::new(GateRegistrator(Arc::clone(gate))),
            owner,
        )
    }

    /// A resolvable binding (no explicit provider namespace) at `home`.
    fn bind(id: &str, type_id: &str, home: &str, owner: Option<Uuid>) -> Trigger {
        Trigger {
            id: id.to_string(),
            trigger_type: type_id.to_string(),
            function_id: format!("fixture::{id}"),
            config: serde_json::json!({}),
            worker_id: owner,
            metadata: None,
            namespace: DEFAULT_NAMESPACE.to_string(),
            trigger_namespace: None,
            home_namespace: home.to_string(),
            provider_namespace: DEFAULT_NAMESPACE.to_string(),
        }
    }

    fn live_on(registry: &TriggerRegistry, id: &str) -> Option<String> {
        registry
            .triggers
            .get(id)
            .map(|t| t.provider_namespace.clone())
    }

    fn spawn_type(
        registry: &Arc<TriggerRegistry>,
        ns: &'static str,
        gate: &Arc<Gate>,
        owner: Uuid,
    ) -> tokio::task::JoinHandle<()> {
        let r = Arc::clone(registry);
        let g = Arc::clone(gate);
        tokio::spawn(async move {
            r.register_trigger_type(gate_type(ns, "evt", &g, Some(owner)))
                .await
                .unwrap()
        })
    }

    fn spawn_cleanup(registry: &Arc<TriggerRegistry>, worker: Uuid) -> tokio::task::JoinHandle<()> {
        let r = Arc::clone(registry);
        tokio::spawn(async move { r.unregister_worker(&worker).await })
    }

    /// Waits up to 3 s for a spawned task. When a shard guard is held across
    /// the paused call (current main), the task may block until the caller
    /// resumes that call. Do NOT await the handle again when this returns true.
    async fn settles(handle: &mut tokio::task::JoinHandle<()>) -> bool {
        match tokio::time::timeout(Duration::from_secs(3), handle).await {
            Ok(joined) => {
                joined.unwrap();
                true
            }
            Err(_) => false,
        }
    }

    /// 1 — `unregister_worker` removes each of the worker's bindings with
    /// `remove_if`, but tears it down on the provider of the SNAPSHOT taken
    /// before the loop. While the loop awaits a provider call, a home provider
    /// re-homes the rest of the worker's bindings off the fallback. The loop
    /// then removes the re-homed entries from the registry but tells the
    /// (stale) fallback to let go: `shop` keeps running them, untracked.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn owner_cleanup_tears_down_on_the_provider_it_removed_from() {
        let registry = Arc::new(TriggerRegistry::new());
        let fallback = Arc::new(Gate::default());
        let other = Arc::new(Gate::default());
        registry
            .register_trigger_type(gate_type(DEFAULT_NAMESPACE, "evt", &fallback, None))
            .await
            .unwrap();
        registry
            .register_trigger_type(gate_type(DEFAULT_NAMESPACE, "other", &other, None))
            .await
            .unwrap();

        let owner = Uuid::new_v4();
        registry
            .register_trigger(bind("a", "other", DEFAULT_NAMESPACE, Some(owner)))
            .await
            .unwrap();
        let ids: Vec<String> = (0..64).map(|i| format!("t{i}")).collect();
        for id in &ids {
            registry
                .register_trigger(bind(id, "evt", "shop", Some(owner)))
                .await
                .unwrap();
        }

        // The owner's cleanup is awaiting `-a` somewhere in its loop.
        other.pause_unregister.store(true, SeqCst);
        let cleanup = spawn_cleanup(&registry, owner);
        other.entered.notified().await;

        // The project's own provider shows up meanwhile and re-homes.
        let home = Arc::new(Gate::default());
        let mut reg = spawn_type(&registry, "shop", &home, Uuid::new_v4());
        let reg_settled = settles(&mut reg).await;
        other.resume.notify_one();
        if !reg_settled {
            reg.await.unwrap();
        }
        cleanup.await.unwrap();

        let tracked: Vec<&String> = ids
            .iter()
            .filter(|id| {
                registry.triggers.contains_key(*id) || registry.pending_triggers.contains_key(*id)
            })
            .collect();
        let ghosts = home.live_ids();
        assert!(
            tracked.is_empty() && ghosts.is_empty(),
            "registry still tracks {tracked:?}; `shop` still runs {} untracked bindings: {ghosts:?}",
            ghosts.len()
        );
    }

    /// 2 — An explicit unregister is awaiting the fallback's `-t` when a home
    /// provider re-homes `t` onto `shop`. The unregister then removes the
    /// re-homed entry from the registry without ever telling `shop` to let go.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn unregister_racing_rehome_does_not_leave_job_on_new_home() {
        let registry = Arc::new(TriggerRegistry::new());
        let fallback = Arc::new(Gate::default());
        registry
            .register_trigger_type(gate_type(DEFAULT_NAMESPACE, "evt", &fallback, None))
            .await
            .unwrap();
        registry
            .register_trigger(bind("t", "evt", "shop", None))
            .await
            .unwrap();

        fallback.pause_unregister.store(true, SeqCst);
        let unreg = {
            let r = Arc::clone(&registry);
            tokio::spawn(async move { r.unregister_trigger("t".to_string(), None).await.unwrap() })
        };
        fallback.entered.notified().await;

        let home = Arc::new(Gate::default());
        let mut reg = spawn_type(&registry, "shop", &home, Uuid::new_v4());
        let reg_settled = settles(&mut reg).await;
        fallback.resume.notify_one();
        if !reg_settled {
            reg.await.unwrap();
        }
        let removed = unreg.await.unwrap();

        assert!(removed);
        let live = live_on(&registry, "t");
        assert!(
            live.is_none() && !registry.pending_triggers.contains_key("t") && !home.live("t"),
            "registry live={live:?}; `shop` runs t={} (untracked); shop events {:?}; \
             fallback events {:?}",
            home.live("t"),
            home.events(),
            fallback.events()
        );
    }

    /// 3 — A parked intent is claimed by the drain (taken out of both maps)
    /// while the provider replay is in flight. An explicit unregister in that
    /// window finds nothing and answers Ok(false); the drain then publishes the
    /// binding: the caller was told it does not exist, yet it fires.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn unregister_during_pending_activation_is_not_lost() {
        let registry = Arc::new(TriggerRegistry::new());
        let outcome = registry
            .register_trigger(bind("t", "evt", DEFAULT_NAMESPACE, None))
            .await
            .unwrap();
        assert!(matches!(outcome, RegisterTriggerOutcome::Deferred));

        let provider = Arc::new(Gate::default());
        provider.pause_register.store(true, SeqCst);
        let reg = spawn_type(&registry, DEFAULT_NAMESPACE, &provider, Uuid::new_v4());
        provider.entered.notified().await;

        let removed = registry
            .unregister_trigger("t".to_string(), None)
            .await
            .unwrap();
        provider.resume.notify_one();
        reg.await.unwrap();

        let live = live_on(&registry, "t");
        assert!(
            live.is_none() && !provider.live("t"),
            "unregister answered removed={removed}, yet registry live={live:?} and the \
             provider runs t={}",
            provider.live("t")
        );
    }

    /// 4 — The home provider leaves; its orphan `t` (owned by worker O) is
    /// taken out of `triggers` and replayed to the fallback. While that replay
    /// is in flight, O disconnects: its cleanup finds `t` in neither map. The
    /// failover then publishes `t` live with a dead owner, and nothing reaps it
    /// later (router_msg only sweeps dead owners out of `pending_triggers`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn owner_disconnect_during_failover_does_not_resurrect_binding() {
        let registry = Arc::new(TriggerRegistry::new());
        let fallback = Arc::new(Gate::default());
        let home = Arc::new(Gate::default());
        let home_worker = Uuid::new_v4();
        let owner = Uuid::new_v4();
        registry
            .register_trigger_type(gate_type(DEFAULT_NAMESPACE, "evt", &fallback, None))
            .await
            .unwrap();
        registry
            .register_trigger_type(gate_type("shop", "evt", &home, Some(home_worker)))
            .await
            .unwrap();
        registry
            .register_trigger(bind("t", "evt", "shop", Some(owner)))
            .await
            .unwrap();
        assert_eq!(live_on(&registry, "t").as_deref(), Some("shop"));

        fallback.pause_register.store(true, SeqCst);
        let provider_cleanup = spawn_cleanup(&registry, home_worker);
        fallback.entered.notified().await;

        let mut owner_cleanup = spawn_cleanup(&registry, owner);
        let settled = settles(&mut owner_cleanup).await;
        fallback.resume.notify_one();
        if !settled {
            owner_cleanup.await.unwrap();
        }
        provider_cleanup.await.unwrap();

        let live = live_on(&registry, "t");
        let pending = registry.pending_triggers.contains_key("t");
        assert!(
            live.is_none() && !pending && !fallback.live("t"),
            "dead owner's binding live={live:?} pending={pending}; fallback runs t={}",
            fallback.live("t")
        );
    }

    async fn fallback_binding(owner: Option<Uuid>) -> (Arc<TriggerRegistry>, Arc<Gate>) {
        let registry = Arc::new(TriggerRegistry::new());
        let fallback = Arc::new(Gate::default());
        registry
            .register_trigger_type(gate_type(DEFAULT_NAMESPACE, "evt", &fallback, None))
            .await
            .unwrap();
        registry
            .register_trigger(bind("t", "evt", "shop", owner))
            .await
            .unwrap();
        (registry, fallback)
    }

    async fn entered(gate: &Gate) {
        tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
            .await
            .expect("provider call reached the gate");
    }

    fn assert_gone(registry: &TriggerRegistry, providers: &[&Gate]) {
        assert!(live_on(registry, "t").is_none());
        assert!(!registry.pending_triggers.contains_key("t"));
        for provider in providers {
            assert!(
                !provider.live("t"),
                "provider still retains t: {:?}",
                provider.events()
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn unregister_during_rehome_activation_does_not_resurrect_binding() {
        let (registry, fallback) = fallback_binding(None).await;
        let home = Arc::new(Gate::default());
        home.pause_register.store(true, SeqCst);
        let registration = spawn_type(&registry, "shop", &home, Uuid::new_v4());
        entered(&home).await;

        assert!(registry.unregister_trigger("t".into(), None).await.unwrap());
        home.resume.notify_one();
        registration.await.unwrap();
        assert_gone(&registry, &[&fallback, &home]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn owner_disconnect_during_pending_activation_cancels_claim() {
        let registry = Arc::new(TriggerRegistry::new());
        let owner = Uuid::new_v4();
        registry
            .register_trigger(bind("t", "evt", DEFAULT_NAMESPACE, Some(owner)))
            .await
            .unwrap();
        let provider = Arc::new(Gate::default());
        provider.pause_register.store(true, SeqCst);
        let registration = spawn_type(&registry, DEFAULT_NAMESPACE, &provider, Uuid::new_v4());
        entered(&provider).await;

        registry.unregister_worker(&owner).await;
        provider.resume.notify_one();
        registration.await.unwrap();
        assert_gone(&registry, &[&provider]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn new_registration_survives_late_rehome() {
        let (registry, fallback) = fallback_binding(None).await;
        let home = Arc::new(Gate::default());
        home.pause_register.store(true, SeqCst);
        let registration = spawn_type(&registry, "shop", &home, Uuid::new_v4());
        entered(&home).await;

        let mut newer = bind("t", "evt", "shop", Some(Uuid::new_v4()));
        newer.function_id = "fixture::newer".into();
        registry.register_trigger(newer).await.unwrap();
        home.resume.notify_one();
        registration.await.unwrap();

        assert_eq!(
            registry.triggers.get("t").unwrap().function_id,
            "fixture::newer"
        );
        assert_eq!(home.function("t").as_deref(), Some("fixture::newer"));
        assert!(!fallback.live("t"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn new_registration_survives_late_unregister_even_with_identical_data() {
        for same_data in [false, true] {
            let (registry, fallback) = fallback_binding(None).await;
            fallback.pause_unregister.store(true, SeqCst);
            let removal = {
                let registry = Arc::clone(&registry);
                tokio::spawn(
                    async move { registry.unregister_trigger("t".into(), None).await.unwrap() },
                )
            };
            entered(&fallback).await;

            let mut newer = bind("t", "evt", "shop", None);
            if !same_data {
                newer.function_id = "fixture::newer".into();
            }
            let expected = newer.function_id.clone();
            registry.register_trigger(newer).await.unwrap();
            fallback.resume.notify_one();
            assert!(removal.await.unwrap());

            assert_eq!(registry.triggers.get("t").unwrap().function_id, expected);
            assert_eq!(fallback.function("t"), Some(expected));
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn new_registration_survives_late_owner_cleanup() {
        let old_owner = Uuid::new_v4();
        let new_owner = Uuid::new_v4();
        let (registry, fallback) = fallback_binding(Some(old_owner)).await;
        fallback.pause_unregister.store(true, SeqCst);
        let cleanup = spawn_cleanup(&registry, old_owner);
        entered(&fallback).await;

        let mut newer = bind("t", "evt", "shop", Some(new_owner));
        newer.function_id = "fixture::newer".into();
        registry.register_trigger(newer).await.unwrap();
        fallback.resume.notify_one();
        cleanup.await.unwrap();

        assert_eq!(
            registry.triggers.get("t").unwrap().worker_id,
            Some(new_owner)
        );
        assert_eq!(fallback.function("t").as_deref(), Some("fixture::newer"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn removal_during_replacement_repair_is_not_lost() {
        let (registry, fallback) = fallback_binding(None).await;
        fallback.pause_unregister.store(true, SeqCst);
        let first_removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(
                async move { registry.unregister_trigger("t".into(), None).await.unwrap() },
            )
        };
        entered(&fallback).await;
        registry
            .register_trigger(bind("t", "evt", "shop", None))
            .await
            .unwrap();

        fallback.pause_register.store(true, SeqCst);
        fallback.resume.notify_one();
        entered(&fallback).await;
        assert!(registry.unregister_trigger("t".into(), None).await.unwrap());
        fallback.resume.notify_one();
        assert!(first_removal.await.unwrap());
        assert_gone(&registry, &[&fallback]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn aborted_caller_does_not_cancel_started_provider_repair() {
        let (registry, fallback) = fallback_binding(None).await;
        fallback.pause_unregister.store(true, SeqCst);
        let removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move { registry.unregister_trigger("t".into(), None).await })
        };
        entered(&fallback).await;
        let mut newer = bind("t", "evt", "shop", None);
        newer.function_id = "fixture::newer".into();
        registry.register_trigger(newer).await.unwrap();
        fallback.pause_register.store(true, SeqCst);
        fallback.resume.notify_one();
        entered(&fallback).await;
        removal.abort();
        assert!(removal.await.unwrap_err().is_cancelled());
        fallback.resume.notify_one();

        tokio::time::timeout(Duration::from_secs(3), async {
            while fallback.function("t").as_deref() != Some("fixture::newer") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("aborting the caller also cancelled its repair");
        assert_eq!(
            registry.triggers.get("t").unwrap().function_id,
            "fixture::newer"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn removal_during_pending_activation_repair_is_not_lost() {
        let registry = Arc::new(TriggerRegistry::new());
        registry
            .register_trigger(bind("t", "evt", DEFAULT_NAMESPACE, None))
            .await
            .unwrap();
        let provider = Arc::new(Gate::default());
        provider.pause_register.store(true, SeqCst);
        let registration = spawn_type(&registry, DEFAULT_NAMESPACE, &provider, Uuid::new_v4());
        entered(&provider).await;

        let mut newer = bind("t", "evt", DEFAULT_NAMESPACE, None);
        newer.function_id = "fixture::newer".into();
        registry.register_trigger(newer).await.unwrap();
        provider.pause_register.store(true, SeqCst);
        provider.resume.notify_one();
        entered(&provider).await;

        assert!(registry.unregister_trigger("t".into(), None).await.unwrap());
        provider.resume.notify_one();
        registration.await.unwrap();
        assert_gone(&registry, &[&provider]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn failed_rehome_detach_keeps_binding_tracked_for_retry() {
        let (registry, fallback) = fallback_binding(None).await;
        fallback.fail_unregister.store(true, SeqCst);
        let home = Arc::new(Gate::default());
        spawn_type(&registry, "shop", &home, Uuid::new_v4())
            .await
            .unwrap();

        assert_eq!(live_on(&registry, "t").as_deref(), Some(DEFAULT_NAMESPACE));
        assert!(fallback.live("t"));
        assert!(!home.live("t"));
        assert!(registry.unregister_trigger("t".into(), None).await.unwrap());
        assert_gone(&registry, &[&fallback, &home]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn failed_unregister_preserves_binding_and_releases_removal_claim() {
        let (registry, fallback) = fallback_binding(None).await;
        fallback.fail_unregister.store(true, SeqCst);
        assert!(registry.unregister_trigger("t".into(), None).await.is_err());
        assert_eq!(live_on(&registry, "t").as_deref(), Some(DEFAULT_NAMESPACE));
        assert!(fallback.live("t"));

        let home = Arc::new(Gate::default());
        spawn_type(&registry, "shop", &home, Uuid::new_v4())
            .await
            .unwrap();
        assert_eq!(live_on(&registry, "t").as_deref(), Some("shop"));
        assert!(!fallback.live("t"));
        assert!(home.live("t"));
        assert!(registry.unregister_trigger("t".into(), None).await.unwrap());
        assert_gone(&registry, &[&fallback, &home]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn aborted_unregister_releases_removal_claim() {
        let (registry, fallback) = fallback_binding(None).await;
        fallback.pause_unregister.store(true, SeqCst);
        let removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move { registry.unregister_trigger("t".into(), None).await })
        };
        entered(&fallback).await;
        removal.abort();
        assert!(removal.await.unwrap_err().is_cancelled());

        let home = Arc::new(Gate::default());
        spawn_type(&registry, "shop", &home, Uuid::new_v4())
            .await
            .unwrap();
        assert_eq!(live_on(&registry, "t").as_deref(), Some("shop"));
        assert!(home.live("t"));
        assert!(!fallback.live("t"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancelled_rehome_rejection_does_not_park_removed_binding() {
        let (registry, fallback) = fallback_binding(None).await;
        let home = Arc::new(Gate::default());
        home.pause_register.store(true, SeqCst);
        home.fail_register.store(true, SeqCst);
        let registration = spawn_type(&registry, "shop", &home, Uuid::new_v4());
        entered(&home).await;

        assert!(registry.unregister_trigger("t".into(), None).await.unwrap());
        home.resume.notify_one();
        registration.await.unwrap();
        assert_gone(&registry, &[&fallback, &home]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn rehome_during_repair_does_not_recreate_old_provider_job() {
        let (registry, fallback) = fallback_binding(None).await;
        fallback.pause_unregister.store(true, SeqCst);
        let removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(
                async move { registry.unregister_trigger("t".into(), None).await.unwrap() },
            )
        };
        entered(&fallback).await;
        registry
            .register_trigger(bind("t", "evt", "shop", None))
            .await
            .unwrap();

        // The stale removal resumes and starts restoring the newer binding.
        fallback.pause_register.store(true, SeqCst);
        fallback.resume.notify_one();
        entered(&fallback).await;

        // Re-home detaches the old provider while that restore waits.
        let home = Arc::new(Gate::default());
        home.pause_register.store(true, SeqCst);
        let registration = spawn_type(&registry, "shop", &home, Uuid::new_v4());
        entered(&home).await;

        // Restore finishes while the live map still points at the old provider.
        fallback.resume.notify_one();
        assert!(removal.await.unwrap());
        home.resume.notify_one();
        registration.await.unwrap();

        assert_eq!(live_on(&registry, "t").as_deref(), Some("shop"));
        assert!(home.live("t"));
        assert!(
            !fallback.live("t"),
            "re-home left a duplicate job on default: {:?}",
            fallback.events()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn same_provider_reannouncement_does_not_lose_replacement_job() {
        let (registry, fallback) = fallback_binding(None).await;
        fallback.pause_unregister.store(true, SeqCst);
        let removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(
                async move { registry.unregister_trigger("t".into(), None).await.unwrap() },
            )
        };
        entered(&fallback).await;
        let mut newer = bind("t", "evt", "shop", None);
        newer.function_id = "fixture::newer".into();
        registry.register_trigger(newer).await.unwrap();

        // Fresh wrapper, same underlying provider. This is a supported re-announce.
        registry
            .register_trigger_type(gate_type(DEFAULT_NAMESPACE, "evt", &fallback, None))
            .await
            .unwrap();
        fallback.resume.notify_one();
        assert!(removal.await.unwrap());

        assert_eq!(
            registry.triggers.get("t").unwrap().function_id,
            "fixture::newer"
        );
        assert_eq!(
            fallback.function("t").as_deref(),
            Some("fixture::newer"),
            "provider lost the live replacement: {:?}",
            fallback.events()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn failed_removal_during_provider_loss_still_fails_over() {
        let (registry, fallback) = fallback_binding(None).await;
        let home = Arc::new(Gate::default());
        let home_owner = Uuid::new_v4();
        spawn_type(&registry, "shop", &home, home_owner)
            .await
            .unwrap();
        home.pause_unregister.store(true, SeqCst);
        home.fail_unregister.store(true, SeqCst);
        let removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move { registry.unregister_trigger("t".into(), None).await })
        };
        entered(&home).await;
        assert!(
            registry
                .remove_trigger_type_if_provider(&("shop".into(), "evt".into()), &home_owner)
                .await
        );
        home.live.lock().unwrap().clear(); // The disconnected provider cannot fire.
        home.resume.notify_one();
        assert!(removal.await.unwrap().is_err());

        assert_eq!(
            live_on(&registry, "t").as_deref(),
            Some(DEFAULT_NAMESPACE),
            "failed removal left the trigger on a disconnected provider"
        );
        assert!(fallback.live("t"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn aborted_removal_during_provider_loss_still_fails_over() {
        let (registry, fallback) = fallback_binding(None).await;
        let home = Arc::new(Gate::default());
        let owner = Uuid::new_v4();
        spawn_type(&registry, "shop", &home, owner).await.unwrap();
        home.pause_unregister.store(true, SeqCst);
        let removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move { registry.unregister_trigger("t".into(), None).await })
        };
        entered(&home).await;
        assert!(
            registry
                .remove_trigger_type_if_provider(&("shop".into(), "evt".into()), &owner)
                .await
        );
        home.live.lock().unwrap().clear();
        removal.abort();
        assert!(removal.await.unwrap_err().is_cancelled());

        tokio::time::timeout(Duration::from_secs(3), async {
            while live_on(&registry, "t").as_deref() != Some(DEFAULT_NAMESPACE)
                || !fallback.live("t")
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("aborted removal did not resume failover");
        assert!(!home.live("t"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn abandoned_removal_recovers_a_replacement_provider_at_the_same_key() {
        for abort in [false, true] {
            let (registry, previous) = fallback_binding(None).await;
            previous.pause_unregister.store(true, SeqCst);
            previous.fail_unregister.store(!abort, SeqCst);
            let removal = {
                let registry = Arc::clone(&registry);
                tokio::spawn(async move { registry.unregister_trigger("t".into(), None).await })
            };
            entered(&previous).await;
            let replacement = Arc::new(Gate::default());
            spawn_type(&registry, DEFAULT_NAMESPACE, &replacement, Uuid::new_v4())
                .await
                .unwrap();
            previous.live.lock().unwrap().clear();
            assert!(!replacement.live("t"));

            if abort {
                removal.abort();
                assert!(removal.await.unwrap_err().is_cancelled());
            } else {
                previous.resume.notify_one();
                assert!(removal.await.unwrap().is_err());
            }
            tokio::time::timeout(Duration::from_secs(3), async {
                while !replacement.live("t") {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("abandoned removal did not restore the replacement provider");
            assert_eq!(live_on(&registry, "t").as_deref(), Some(DEFAULT_NAMESPACE));
            assert_eq!(replacement.function("t").as_deref(), Some("fixture::t"));
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn aborted_removal_does_not_recover_over_a_newer_binding() {
        let (registry, previous) = fallback_binding(None).await;
        previous.pause_unregister.store(true, SeqCst);
        let removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move { registry.unregister_trigger("t".into(), None).await })
        };
        entered(&previous).await;
        let replacement = Arc::new(Gate::default());
        spawn_type(&registry, DEFAULT_NAMESPACE, &replacement, Uuid::new_v4())
            .await
            .unwrap();
        let mut newer = bind("t", "evt", "shop", Some(Uuid::new_v4()));
        newer.function_id = "fixture::newer".into();
        registry.register_trigger(newer).await.unwrap();
        removal.abort();
        assert!(removal.await.unwrap_err().is_cancelled());

        assert_eq!(
            registry.triggers.get("t").unwrap().function_id,
            "fixture::newer"
        );
        assert_eq!(replacement.function("t").as_deref(), Some("fixture::newer"));
        assert_eq!(replacement.events(), vec!["+t"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn pending_repair_after_same_provider_reannouncement_keeps_new_job() {
        let registry = Arc::new(TriggerRegistry::new());
        registry
            .register_trigger(bind("t", "evt", DEFAULT_NAMESPACE, None))
            .await
            .unwrap();
        let provider = Arc::new(Gate::default());
        provider.pause_register.store(true, SeqCst);
        let provider_owner = Uuid::new_v4();
        let registration = spawn_type(&registry, DEFAULT_NAMESPACE, &provider, provider_owner);
        entered(&provider).await;

        let mut newer = bind("t", "evt", DEFAULT_NAMESPACE, None);
        newer.function_id = "fixture::newer".into();
        registry.register_trigger(newer).await.unwrap();
        registry
            .register_trigger_type(gate_type(
                DEFAULT_NAMESPACE,
                "evt",
                &provider,
                Some(provider_owner),
            ))
            .await
            .unwrap();

        provider.resume.notify_one();
        registration.await.unwrap();
        assert_eq!(
            registry.triggers.get("t").unwrap().function_id,
            "fixture::newer"
        );
        assert_eq!(
            provider.function("t").as_deref(),
            Some("fixture::newer"),
            "pending compensation lost the newer job: {:?}",
            provider.events()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn failed_detach_after_source_repair_keeps_source_job() {
        let (registry, fallback) = fallback_binding(None).await;
        fallback.pause_unregister.store(true, SeqCst);
        let removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(
                async move { registry.unregister_trigger("t".into(), None).await.unwrap() },
            )
        };
        entered(&fallback).await;
        registry
            .register_trigger(bind("t", "evt", "shop", None))
            .await
            .unwrap();
        fallback.pause_register.store(true, SeqCst);
        fallback.resume.notify_one();
        entered(&fallback).await;

        // A second paused call now waits behind the repair in Notify's FIFO.
        fallback.pause_unregister.store(true, SeqCst);
        let home = Arc::new(Gate::default());
        let registration = spawn_type(&registry, "shop", &home, Uuid::new_v4());
        entered(&fallback).await;
        fallback.resume.notify_one();
        assert!(removal.await.unwrap());

        fallback.fail_unregister.store(true, SeqCst);
        fallback.resume.notify_one();
        registration.await.unwrap();
        assert_eq!(live_on(&registry, "t").as_deref(), Some(DEFAULT_NAMESPACE));
        assert!(
            fallback.live("t"),
            "failed move left its source job missing: {:?}",
            fallback.events()
        );
        assert!(!home.live("t"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn aborted_rehome_after_source_repair_restores_source_job() {
        let (registry, fallback) = fallback_binding(None).await;
        fallback.pause_unregister.store(true, SeqCst);
        let removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(
                async move { registry.unregister_trigger("t".into(), None).await.unwrap() },
            )
        };
        entered(&fallback).await;
        registry
            .register_trigger(bind("t", "evt", "shop", None))
            .await
            .unwrap();
        fallback.pause_register.store(true, SeqCst);
        fallback.resume.notify_one();
        entered(&fallback).await;

        fallback.pause_unregister.store(true, SeqCst);
        let home = Arc::new(Gate::default());
        let registration = spawn_type(&registry, "shop", &home, Uuid::new_v4());
        entered(&fallback).await;
        fallback.resume.notify_one();
        assert!(removal.await.unwrap());
        assert!(!fallback.live("t"));

        registration.abort();
        assert!(registration.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(3), async {
            while !fallback.live("t") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("aborted move did not restore its source");
        assert_eq!(live_on(&registry, "t").as_deref(), Some(DEFAULT_NAMESPACE));
        assert!(!home.live("t"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn failed_new_activation_cleans_job_created_by_old_repair() {
        let registry = Arc::new(TriggerRegistry::new());
        registry
            .register_trigger(bind("t", "evt", DEFAULT_NAMESPACE, None))
            .await
            .unwrap();
        let old = Arc::new(Gate::default());
        let old_owner = Uuid::new_v4();
        old.pause_register.store(true, SeqCst);
        let old_activation = spawn_type(&registry, DEFAULT_NAMESPACE, &old, old_owner);
        entered(&old).await;

        assert!(
            registry
                .remove_trigger_type_if_provider(
                    &(DEFAULT_NAMESPACE.into(), "evt".into()),
                    &old_owner
                )
                .await
        );
        let mut newer = bind("t", "evt", DEFAULT_NAMESPACE, None);
        newer.function_id = "fixture::newer".into();
        registry.register_trigger(newer).await.unwrap();
        let new = Arc::new(Gate::default());
        new.pause_register.store(true, SeqCst);
        let new_activation = spawn_type(&registry, DEFAULT_NAMESPACE, &new, Uuid::new_v4());
        entered(&new).await;

        old.resume.notify_one();
        old_activation.await.unwrap();
        new.fail_register.store(true, SeqCst);
        new.resume.notify_one();
        new_activation.await.unwrap();

        assert!(live_on(&registry, "t").is_none());
        assert!(registry.pending_triggers.contains_key("t"));
        assert!(!old.live("t"));
        assert!(
            !new.live("t"),
            "failed activation left a job created by compensation: {:?}",
            new.events()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn failed_removal_after_rejection_allows_new_provider_activation() {
        let registry = Arc::new(TriggerRegistry::new());
        let old = Arc::new(Gate::default());
        let old_owner = Uuid::new_v4();
        spawn_type(&registry, DEFAULT_NAMESPACE, &old, old_owner)
            .await
            .unwrap();
        registry
            .register_trigger(bind("t", "evt", DEFAULT_NAMESPACE, None))
            .await
            .unwrap();
        old.pause_unregister.store(true, SeqCst);
        old.fail_unregister.store(true, SeqCst);
        let removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move { registry.unregister_trigger("t".into(), None).await })
        };
        entered(&old).await;

        registry.park_rejected_trigger("t", old_owner).await;
        old.live.lock().unwrap().clear();
        let new = Arc::new(Gate::default());
        spawn_type(&registry, DEFAULT_NAMESPACE, &new, Uuid::new_v4())
            .await
            .unwrap();
        old.resume.notify_one();
        assert!(removal.await.unwrap().is_err());

        assert!(
            !registry.pending_triggers.contains_key("t"),
            "removal failure left an activatable binding stranded in pending"
        );
        assert_eq!(live_on(&registry, "t").as_deref(), Some(DEFAULT_NAMESPACE));
        assert!(new.live("t"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn aborted_new_activation_cleans_job_created_by_old_repair() {
        let registry = Arc::new(TriggerRegistry::new());
        registry
            .register_trigger(bind("t", "evt", DEFAULT_NAMESPACE, None))
            .await
            .unwrap();
        let old = Arc::new(Gate::default());
        let old_owner = Uuid::new_v4();
        old.pause_register.store(true, SeqCst);
        let old_activation = spawn_type(&registry, DEFAULT_NAMESPACE, &old, old_owner);
        entered(&old).await;

        assert!(
            registry
                .remove_trigger_type_if_provider(
                    &(DEFAULT_NAMESPACE.into(), "evt".into()),
                    &old_owner
                )
                .await
        );
        let mut newer = bind("t", "evt", DEFAULT_NAMESPACE, None);
        newer.function_id = "fixture::newer".into();
        registry.register_trigger(newer).await.unwrap();
        let new = Arc::new(Gate::default());
        new.pause_register.store(true, SeqCst);
        let new_activation = spawn_type(&registry, DEFAULT_NAMESPACE, &new, Uuid::new_v4());
        entered(&new).await;

        old.resume.notify_one();
        old_activation.await.unwrap();
        new_activation.abort();
        assert!(new_activation.await.unwrap_err().is_cancelled());

        assert!(live_on(&registry, "t").is_none());
        assert!(!old.live("t"));
        assert!(registry.pending_triggers.contains_key("t"));
        tokio::time::timeout(Duration::from_secs(3), async {
            while new.live("t") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("aborted activation left a job created by compensation");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn aborted_error_cleanup_still_removes_provisional_job() {
        let registry = Arc::new(TriggerRegistry::new());
        registry
            .register_trigger(bind("t", "evt", DEFAULT_NAMESPACE, None))
            .await
            .unwrap();
        let old = Arc::new(Gate::default());
        let old_owner = Uuid::new_v4();
        old.pause_register.store(true, SeqCst);
        let old_activation = spawn_type(&registry, DEFAULT_NAMESPACE, &old, old_owner);
        entered(&old).await;

        assert!(
            registry
                .remove_trigger_type_if_provider(
                    &(DEFAULT_NAMESPACE.into(), "evt".into()),
                    &old_owner
                )
                .await
        );
        let mut newer = bind("t", "evt", DEFAULT_NAMESPACE, None);
        newer.function_id = "fixture::newer".into();
        registry.register_trigger(newer).await.unwrap();
        let new = Arc::new(Gate::default());
        new.pause_register.store(true, SeqCst);
        let new_activation = spawn_type(&registry, DEFAULT_NAMESPACE, &new, Uuid::new_v4());
        entered(&new).await;

        old.resume.notify_one();
        old_activation.await.unwrap();
        new.fail_register.store(true, SeqCst);
        new.pause_unregister.store(true, SeqCst);
        new.resume.notify_one();
        entered(&new).await;
        new_activation.abort();
        assert!(new_activation.await.unwrap_err().is_cancelled());
        new.resume.notify_one();
        tokio::time::timeout(Duration::from_secs(3), async {
            while new.live("t") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("aborting the caller also cancelled its provider cleanup");

        assert!(live_on(&registry, "t").is_none());
        assert!(registry.pending_triggers.contains_key("t"));
        assert!(!old.live("t"));
        assert!(
            !new.live("t"),
            "failed activation left a job created by compensation: {:?}",
            new.events()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn unregister_then_abort_cleans_provisional_job() {
        let registry = Arc::new(TriggerRegistry::new());
        registry
            .register_trigger(bind("t", "evt", DEFAULT_NAMESPACE, None))
            .await
            .unwrap();
        let old = Arc::new(Gate::default());
        let old_owner = Uuid::new_v4();
        old.pause_register.store(true, SeqCst);
        let old_activation = spawn_type(&registry, DEFAULT_NAMESPACE, &old, old_owner);
        entered(&old).await;

        assert!(
            registry
                .remove_trigger_type_if_provider(
                    &(DEFAULT_NAMESPACE.into(), "evt".into()),
                    &old_owner
                )
                .await
        );
        let mut newer = bind("t", "evt", DEFAULT_NAMESPACE, None);
        newer.function_id = "fixture::newer".into();
        registry.register_trigger(newer).await.unwrap();
        let new = Arc::new(Gate::default());
        new.pause_register.store(true, SeqCst);
        let new_activation = spawn_type(&registry, DEFAULT_NAMESPACE, &new, Uuid::new_v4());
        entered(&new).await;

        old.resume.notify_one();
        old_activation.await.unwrap();
        assert!(registry.unregister_trigger("t".into(), None).await.unwrap());
        new_activation.abort();
        assert!(new_activation.await.unwrap_err().is_cancelled());

        assert!(live_on(&registry, "t").is_none());
        assert!(!old.live("t"));
        assert!(!registry.pending_triggers.contains_key("t"));
        tokio::time::timeout(Duration::from_secs(3), async {
            while new.live("t") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("unregister then abort left a job created by compensation");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn aborted_removal_after_other_repair_restores_live_job() {
        let (registry, fallback) = fallback_binding(None).await;
        fallback.pause_unregister.store(true, SeqCst);
        let first_removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(
                async move { registry.unregister_trigger("t".into(), None).await.unwrap() },
            )
        };
        entered(&fallback).await;
        registry
            .register_trigger(bind("t", "evt", "shop", None))
            .await
            .unwrap();
        fallback.pause_register.store(true, SeqCst);
        fallback.resume.notify_one();
        entered(&fallback).await;

        fallback.pause_unregister.store(true, SeqCst);
        let second_removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(
                async move { registry.unregister_trigger("t".into(), None).await.unwrap() },
            )
        };
        entered(&fallback).await;
        fallback.resume.notify_one();
        assert!(first_removal.await.unwrap());
        assert!(!fallback.live("t"));
        second_removal.abort();
        assert!(second_removal.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(3), async {
            while !fallback.live("t") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("aborted removal did not restore the job removed by concurrent repair");
        assert_eq!(live_on(&registry, "t").as_deref(), Some(DEFAULT_NAMESPACE));
        assert!(!registry.pending_triggers.contains_key("t"));
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn abort_between_rehome_repairs_still_restores_source() {
        let (registry, fallback) = fallback_binding(None).await;
        fallback.pause_unregister.store(true, SeqCst);
        let removal = {
            let registry = Arc::clone(&registry);
            tokio::spawn(
                async move { registry.unregister_trigger("t".into(), None).await.unwrap() },
            )
        };
        entered(&fallback).await;
        registry
            .register_trigger(bind("t", "evt", "shop", None))
            .await
            .unwrap();
        fallback.pause_register.store(true, SeqCst);
        fallback.resume.notify_one();
        entered(&fallback).await;

        // A second paused call now waits behind the repair in Notify's FIFO.
        fallback.pause_unregister.store(true, SeqCst);
        let home = Arc::new(Gate::default());
        let registration = spawn_type(&registry, "shop", &home, Uuid::new_v4());
        entered(&fallback).await;
        fallback.resume.notify_one();
        assert!(removal.await.unwrap());

        fallback.fail_unregister.store(true, SeqCst);
        home.pause_unregister.store(true, SeqCst);
        fallback.resume.notify_one();
        entered(&home).await;
        registration.abort();
        assert!(registration.await.unwrap_err().is_cancelled());
        home.resume.notify_one();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !fallback.live("t") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("aborting between destination and source repair lost the source job");
        assert_eq!(live_on(&registry, "t").as_deref(), Some(DEFAULT_NAMESPACE));
        assert!(
            fallback.live("t"),
            "failed move left its source job missing: {:?}",
            fallback.events()
        );
        assert!(!home.live("t"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancelled_rehome_before_detach_completion_cleans_provisional_destination() {
        let registry = Arc::new(TriggerRegistry::new());
        registry
            .register_trigger(bind("t", "evt", "shop", None))
            .await
            .unwrap();
        let old_home = Arc::new(Gate::default());
        let old_owner = Uuid::new_v4();
        old_home.pause_register.store(true, SeqCst);
        let old_activation = spawn_type(&registry, "shop", &old_home, old_owner);
        entered(&old_home).await;

        let fallback = Arc::new(Gate::default());
        spawn_type(&registry, DEFAULT_NAMESPACE, &fallback, Uuid::new_v4())
            .await
            .unwrap();
        assert!(
            registry
                .remove_trigger_type_if_provider(&("shop".into(), "evt".into()), &old_owner)
                .await
        );
        registry
            .register_trigger(bind("t", "evt", "shop", None))
            .await
            .unwrap();

        fallback.pause_unregister.store(true, SeqCst);
        let new_home = Arc::new(Gate::default());
        let rehome = spawn_type(&registry, "shop", &new_home, Uuid::new_v4());
        entered(&fallback).await;
        old_home.resume.notify_one();
        old_activation.await.unwrap();
        assert!(
            new_home.live("t"),
            "old activation's repair delivered the new provisional target"
        );

        assert!(registry.unregister_trigger("t".into(), None).await.unwrap());
        fallback.resume.notify_one();
        rehome.await.unwrap();
        assert_gone(&registry, &[&old_home, &fallback, &new_home]);
    }
}
