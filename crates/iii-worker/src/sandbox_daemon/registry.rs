//! In-memory registry of active sandboxes. Tracks UUID -> SandboxState
//! with a bounded in-flight exec count for admission control.

use crate::sandbox_daemon::errors::SandboxError;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SandboxState {
    pub id: Uuid,
    pub name: Option<String>,
    pub image: String,
    pub rootfs: PathBuf,
    pub workdir: PathBuf,
    pub shell_sock: PathBuf,
    pub vm_pid: Option<u32>,
    /// Write end of the VM's lifeline pipe (see `daemon_exit::Lifeline`).
    /// While any clone of this state is alive the VM keeps running; once the
    /// entry is dropped — or this whole daemon dies, however abruptly — the
    /// pipe closes and `__vm-boot` self-terminates.
    pub lifeline: Option<std::sync::Arc<crate::daemon_exit::Lifeline>>,
    pub created_at: Instant,
    pub last_exec_at: Instant,
    /// How many execs are running in this sandbox right now.
    ///
    /// Was a `bool` — one exec at a time, a second rejected with S003. That
    /// was daemon POLICY, not a transport limit: both ends of the `iii.exec`
    /// channel already multiplex by `corr_id` (host `cli::shell_relay`,
    /// guest `iii_init::shell_dispatcher`), each session with its own threads
    /// and a shared writer that keeps frames from interleaving.
    ///
    /// A COUNT rather than an unbounded free-for-all because the guest is
    /// small: `default_cpus` is 1 and `default_memory_mb` 512, so enough
    /// simultaneous interpreters OOM the VM long before they finish.
    /// [`SandboxRegistry::begin_exec`] admits up to a caller-supplied cap and
    /// still returns S003 above it.
    ///
    /// The count also tells the reaper a sandbox is busy — see
    /// `reaper::run_reaper_loop`, which used to reap mid-exec because the
    /// bool recording that was consulted by nobody.
    pub exec_in_flight: u32,
    pub idle_timeout_secs: u64,
    pub stopped: bool,
}

impl SandboxState {
    /// Whether any exec is running. `sandbox::list`'s wire field stayed a
    /// bool when the internal record became a count.
    pub fn exec_in_progress(&self) -> bool {
        self.exec_in_flight > 0
    }
}

/// Concurrent execs admitted per sandbox when the registry is built without
/// an explicit cap. Mirrors `SandboxConfig::max_concurrent_exec_per_sandbox`;
/// the daemon overrides it from config at startup.
pub const DEFAULT_MAX_EXEC_IN_FLIGHT: u32 = 4;

/// Path of a sandbox's network-activity beacon, derived from its shell
/// socket the same way the launcher derives `control.sock` and
/// `vm-boot.stderr.log`. Single source of truth for the file name: the
/// launcher passes it to `__vm-boot` (whose smoltcp stack refreshes the
/// mtime on relayed guest payload) and the reaper stats it. The file lives
/// in the sandbox's overlay base dir, so `OverlayLayout::cleanup` reclaims
/// it with everything else.
pub fn net_activity_path(shell_sock: &std::path::Path) -> std::path::PathBuf {
    shell_sock.with_file_name("net-activity")
}

#[derive(Clone)]
pub struct SandboxRegistry {
    inner: Arc<Mutex<HashMap<Uuid, SandboxState>>>,
    /// Admission cap, held here rather than threaded through `begin_exec`'s
    /// ~28 call sites. 0 is treated as 1 by `begin_exec`, so a `Default`
    /// registry serializes rather than deadlocking.
    max_exec_in_flight: u32,
}

impl Default for SandboxRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SandboxRegistry {
    pub fn new() -> Self {
        Self::with_max_exec_in_flight(DEFAULT_MAX_EXEC_IN_FLIGHT)
    }

    /// Registry whose sandboxes admit at most `max` concurrent execs.
    pub fn with_max_exec_in_flight(max: u32) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            max_exec_in_flight: max,
        }
    }

    pub async fn insert(&self, state: SandboxState) {
        let mut map = self.inner.lock().await;
        map.insert(state.id, state);
    }

    pub async fn get(&self, id: Uuid) -> Result<SandboxState, SandboxError> {
        let map = self.inner.lock().await;
        map.get(&id)
            .cloned()
            .ok_or_else(|| SandboxError::NotFound(id.to_string()))
    }

    /// Admit one exec, up to the registry's concurrency cap. Returns S003
    /// when the sandbox is already at that cap.
    ///
    /// A cap of 0 is treated as 1 — admitting nothing would wedge the
    /// sandbox permanently, which is never what an operator means by a
    /// limit.
    pub async fn begin_exec(&self, id: Uuid) -> Result<SandboxState, SandboxError> {
        let mut map = self.inner.lock().await;
        let state = map
            .get_mut(&id)
            .ok_or_else(|| SandboxError::NotFound(id.to_string()))?;
        if state.stopped {
            return Err(SandboxError::AlreadyStopped(id.to_string()));
        }
        if state.exec_in_flight >= self.max_exec_in_flight.max(1) {
            return Err(SandboxError::ConcurrentExec(id.to_string()));
        }
        state.exec_in_flight += 1;
        state.last_exec_at = Instant::now();
        Ok(state.clone())
    }

    pub async fn end_exec(&self, id: Uuid) {
        let mut map = self.inner.lock().await;
        if let Some(state) = map.get_mut(&id) {
            // Saturating: an end without a matching begin must not wrap to
            // u32::MAX and lock the sandbox out of every future exec.
            state.exec_in_flight = state.exec_in_flight.saturating_sub(1);
            state.last_exec_at = Instant::now();
        }
    }

    /// Decide under ONE lock whether `id` should be reaped, and if so mark it
    /// stopped so nothing else can claim it. `None` means leave it alone.
    ///
    /// Atomic on purpose. The reaper used to filter a `list()` snapshot and
    /// then act on it, so an exec admitted between the snapshot and the reap
    /// was still killed mid-run — the very defect the busy-check was added to
    /// prevent. The window is not tight: each preceding reap sleeps
    /// `STOP_GRACE_MS` and does overlay cleanup I/O.
    ///
    /// `busy_grace` bounds the busy exemption. Skipping busy sandboxes
    /// outright removed the unconditional reclamation backstop: one exec —
    /// with a leaked count, a wedged child, or simply a long deadline — could
    /// pin a VM forever, and 32 of those exhaust `max_concurrent_sandboxes`
    /// until the daemon restarts. Past `idle_timeout + busy_grace` no
    /// legitimate exec can still be running (the runner clamps every deadline
    /// to `max_exec_timeout_ms`), so the count is leaked or the child is
    /// wedged, and reaping is the correct call.
    ///
    /// `net_idle` is the time since the sandbox's network-activity beacon
    /// ([`net_activity_path`]) was last refreshed; `None` means no beacon
    /// exists (networking off, or its creation failed). The idle basis is
    /// the most recent activity of ANY kind — `min(exec idle, net idle)` —
    /// so a sandbox serving pure network traffic is no longer reaped
    /// mid-service. Unlike `exec_in_flight` this input needs no leak bound:
    /// a timestamp ages out by itself the moment traffic stops.
    ///
    /// The beacon is stat'ed by the caller BEFORE this lock is taken. That
    /// race is irreducible, not sloppy: network activity is produced by
    /// another process and cannot be admission-controlled under this lock
    /// the way execs are — payload can land between any decision point and
    /// the SIGTERM. The stat-to-claim window (microseconds, no awaits)
    /// merely narrows a boundary that is inherently fuzzy at the timeout
    /// edge.
    pub async fn try_claim_for_reap(
        &self,
        id: Uuid,
        now: Instant,
        busy_grace: std::time::Duration,
        net_idle: Option<std::time::Duration>,
    ) -> Option<SandboxState> {
        let mut map = self.inner.lock().await;
        let state = map.get_mut(&id)?;
        if state.stopped {
            return None;
        }
        let mut idle = now.saturating_duration_since(state.last_exec_at);
        if let Some(net) = net_idle {
            idle = idle.min(net);
        }
        if idle <= std::time::Duration::from_secs(state.idle_timeout_secs) {
            return None;
        }
        if state.exec_in_flight > 0
            && idle <= std::time::Duration::from_secs(state.idle_timeout_secs) + busy_grace
        {
            return None;
        }
        state.stopped = true;
        Some(state.clone())
    }

    pub async fn mark_stopped(&self, id: Uuid) {
        let mut map = self.inner.lock().await;
        if let Some(state) = map.get_mut(&id) {
            state.stopped = true;
        }
    }

    pub async fn remove(&self, id: Uuid) -> Option<SandboxState> {
        let mut map = self.inner.lock().await;
        map.remove(&id)
    }

    pub async fn list(&self) -> Vec<SandboxState> {
        let map = self.inner.lock().await;
        map.values().cloned().collect()
    }

    /// The per-sandbox exec admission cap this registry enforces.
    pub fn max_exec_in_flight(&self) -> u32 {
        self.max_exec_in_flight.max(1)
    }

    pub async fn count(&self) -> usize {
        self.inner.lock().await.len()
    }

    /// Update `last_exec_at` to now. Called by fs::* trigger handlers (which
    /// don't use the exec-serialization guard) to keep the idle reaper from
    /// evicting a sandbox that is actively performing file operations.
    pub async fn bump_last_exec(&self, id: Uuid) {
        let mut map = self.inner.lock().await;
        if let Some(state) = map.get_mut(&id) {
            state.last_exec_at = Instant::now();
        }
    }
}

/// A `SandboxState` for tests, with every field at a sane default.
///
/// `pub` rather than `#[cfg(test)]` because the integration tests under
/// `tests/` link the compiled lib and cannot see a test-only item. It exists
/// because the same ~12-line literal was copy-pasted into 18 test modules —
/// a single field change meant an 18-file mechanical edit, and the copies had
/// already drifted. Callers mutate what they care about:
///
/// ```ignore
/// let mut s = sandbox_state_for_test(id);
/// s.idle_timeout_secs = 5;
/// ```
#[doc(hidden)]
pub fn sandbox_state_for_test(id: Uuid) -> SandboxState {
    SandboxState {
        id,
        name: None,
        image: "python".into(),
        rootfs: PathBuf::from("/tmp/r"),
        workdir: PathBuf::from("/tmp/w"),
        shell_sock: PathBuf::from("/tmp/s"),
        vm_pid: Some(1),
        lifeline: None,
        created_at: Instant::now(),
        last_exec_at: Instant::now(),
        exec_in_flight: 0,
        idle_timeout_secs: 300,
        stopped: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn fixture(id: Uuid) -> SandboxState {
        SandboxState {
            id,
            name: None,
            image: "python".into(),
            rootfs: PathBuf::from("/tmp/rootfs"),
            workdir: PathBuf::from("/tmp/work"),
            shell_sock: PathBuf::from("/tmp/sock"),
            vm_pid: Some(1234),
            lifeline: None,
            created_at: Instant::now(),
            last_exec_at: Instant::now(),
            exec_in_flight: 0,
            idle_timeout_secs: 300,
            stopped: false,
        }
    }

    #[tokio::test]
    async fn insert_and_get() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        let s = reg.get(id).await.unwrap();
        assert_eq!(s.id, id);
    }

    #[tokio::test]
    async fn get_missing_returns_s002() {
        let reg = SandboxRegistry::new();
        let err = reg.get(Uuid::new_v4()).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S002");
    }

    #[tokio::test]
    async fn begin_exec_marks_in_progress() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        let _ = reg.begin_exec(id).await.unwrap();
        let s = reg.get(id).await.unwrap();
        assert!(s.exec_in_progress());
        assert_eq!(s.exec_in_flight, 1);
    }

    /// The change this file exists to make: a second exec no longer bounces.
    /// Both ends of the exec channel multiplex by `corr_id`; only this
    /// registry ever refused.
    #[tokio::test]
    async fn concurrent_execs_are_admitted_up_to_the_cap() {
        let reg = SandboxRegistry::with_max_exec_in_flight(4);
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        for expected in 1..=4 {
            reg.begin_exec(id).await.expect("under the cap");
            assert_eq!(reg.get(id).await.unwrap().exec_in_flight, expected);
        }
    }

    /// Unbounded would OOM the guest: it defaults to 1 vCPU and 512 MB, so
    /// enough simultaneous interpreters kill the VM rather than queueing.
    #[tokio::test]
    async fn exceeding_the_cap_still_returns_s003_with_actionable_recovery() {
        let reg = SandboxRegistry::with_max_exec_in_flight(2);
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        let _ = reg.begin_exec(id).await.unwrap();
        let _ = reg.begin_exec(id).await.unwrap();
        let err = reg.begin_exec(id).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S003");
        // The recovery hint must NOT tell the agent to merely "wait" — that
        // loops forever on a foreground server. It must name detach/stop and
        // flag the foreground case.
        let note = err
            .to_payload()
            .get("fix_note")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        assert!(
            note.contains("nohup") || note.contains("sandbox::stop"),
            "fix_note must name a real recovery (detach/stop), got: {note}"
        );
        assert!(
            note.to_lowercase().contains("foreground"),
            "fix_note must flag the foreground case, got: {note}"
        );
    }

    #[tokio::test]
    async fn end_exec_clears_flag() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        let _ = reg.begin_exec(id).await.unwrap();
        reg.end_exec(id).await;
        let _ = reg.begin_exec(id).await.unwrap();
    }

    #[tokio::test]
    async fn stopped_sandbox_rejects_exec_with_s004() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        reg.mark_stopped(id).await;
        let err = reg.begin_exec(id).await.unwrap_err();
        assert_eq!(err.code().as_str(), "S004");
    }

    /// A cap of 0 must serialize, not wedge the sandbox forever.
    #[tokio::test]
    async fn a_zero_cap_admits_one_rather_than_none() {
        let reg = SandboxRegistry::with_max_exec_in_flight(0);
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        reg.begin_exec(id).await.expect("0 is treated as 1");
        assert!(reg.begin_exec(id).await.is_err());
    }

    /// An unmatched `end_exec` must not wrap the counter to u32::MAX and lock
    /// the sandbox out of every future exec.
    #[tokio::test]
    async fn end_exec_without_begin_does_not_underflow() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        reg.end_exec(id).await;
        assert_eq!(reg.get(id).await.unwrap().exec_in_flight, 0);
        reg.begin_exec(id).await.expect("still admits execs");
    }

    /// A sandbox way past its exec-idle timeout but with fresh network
    /// activity is NOT reapable — `min(exec idle, net idle)` is the basis.
    #[tokio::test]
    async fn fresh_network_activity_defers_reaping() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        let mut s = fixture(id);
        s.last_exec_at = Instant::now() - std::time::Duration::from_secs(600);
        s.idle_timeout_secs = 300;
        reg.insert(s).await;

        let claimed = reg
            .try_claim_for_reap(
                id,
                Instant::now(),
                std::time::Duration::from_secs(3600),
                Some(std::time::Duration::from_secs(5)), // net payload 5s ago
            )
            .await;
        assert!(claimed.is_none(), "fresh net activity must defer the reap");
        assert!(
            !reg.get(id).await.unwrap().stopped,
            "a deferred sandbox must not be marked stopped"
        );
    }

    /// ...and once the network has been silent past the timeout too, the
    /// sandbox is genuinely idle and is claimed.
    #[tokio::test]
    async fn stale_network_activity_does_not_defer_reaping() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        let mut s = fixture(id);
        s.last_exec_at = Instant::now() - std::time::Duration::from_secs(600);
        s.idle_timeout_secs = 300;
        reg.insert(s).await;

        let claimed = reg
            .try_claim_for_reap(
                id,
                Instant::now(),
                std::time::Duration::from_secs(3600),
                Some(std::time::Duration::from_secs(600)),
            )
            .await;
        assert!(claimed.is_some(), "stale on both clocks => reapable");
    }

    /// No beacon (networking off / old VM): behavior is exactly the
    /// pre-beacon exec-idle rule.
    #[tokio::test]
    async fn absent_beacon_reaps_on_exec_idleness_alone() {
        let reg = SandboxRegistry::new();
        let id = Uuid::new_v4();
        let mut s = fixture(id);
        s.last_exec_at = Instant::now() - std::time::Duration::from_secs(600);
        s.idle_timeout_secs = 300;
        reg.insert(s).await;

        let claimed = reg
            .try_claim_for_reap(
                id,
                Instant::now(),
                std::time::Duration::from_secs(3600),
                None,
            )
            .await;
        assert!(claimed.is_some());
    }

    #[tokio::test]
    async fn end_exec_releases_one_slot_not_all() {
        let reg = SandboxRegistry::with_max_exec_in_flight(3);
        let id = Uuid::new_v4();
        reg.insert(fixture(id)).await;
        reg.begin_exec(id).await.unwrap();
        reg.begin_exec(id).await.unwrap();
        reg.end_exec(id).await;
        assert_eq!(reg.get(id).await.unwrap().exec_in_flight, 1);
        assert!(reg.get(id).await.unwrap().exec_in_progress());
    }
}
