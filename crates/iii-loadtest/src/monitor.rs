// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Samples the engine every 250 ms and says what it cost over a window.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::sample::{self, Sample};

const INTERVAL: Duration = Duration::from_millis(250);

#[derive(Default)]
struct State {
    samples: Vec<(Instant, Sample)>,
    exited: bool,
    /// OOM kills in the engine's cgroup, read even once the engine is gone.
    oom_kills: u64,
}

pub struct Monitor {
    state: Mutex<State>,
    changed: Notify,
    sampler: Mutex<Option<tokio::task::AbortHandle>>,
}

/// What the engine (and the CPUs it runs on) cost over a window.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Resources {
    /// Cores the engine process used on average (1.00 = one CPU busy).
    pub engine_cpu: f64,
    /// Cores of the engine's CPUs that were busy: the engine plus the
    /// kernel's work on them (network softirqs, page faults).
    pub cpus_busy: f64,
    /// CPUs the engine may run on.
    pub cpus: usize,
    pub rss_avg_mb: f64,
    pub rss_max_mb: f64,
    /// Peak RSS of the engine since it started (`VmHWM`).
    pub hwm_mb: f64,
    pub threads_max: u64,
    /// The engine's cgroup peak (page cache included), when it has one.
    pub cgroup_peak_mb: Option<f64>,
    pub machine_mem_mb: f64,
    /// The least memory the machine had available in the window.
    pub machine_mem_available_min_mb: f64,
    /// Cores the load generator used.
    pub loadgen_cpu: f64,
    pub samples: usize,
}

impl Monitor {
    /// Starts sampling the engine with `pid`, pinned to `cpus`, charged to
    /// `cgroup` when it has its own.
    pub fn start(pid: u32, cpus: Vec<usize>, cgroup: Option<PathBuf>) -> Arc<Self> {
        let monitor = Arc::new(Monitor {
            state: Mutex::new(State::default()),
            changed: Notify::new(),
            sampler: Mutex::new(None),
        });
        let weak = Arc::downgrade(&monitor);
        let started = Instant::now();
        let task = tokio::spawn(async move {
            let mut tick = tokio::time::interval(INTERVAL);
            loop {
                tick.tick().await;
                let Some(monitor) = weak.upgrade() else {
                    return;
                };
                let t_ms = started.elapsed().as_secs_f64() * 1000.0;
                let sample = sample::read(pid, t_ms, &cpus, cgroup.as_deref());
                let ooms = cgroup.as_deref().and_then(sample::oom_kills);
                let mut s = monitor.state.lock().unwrap();
                s.oom_kills = s.oom_kills.max(ooms.unwrap_or(0));
                match sample {
                    Some(sample) => s.samples.push((Instant::now(), sample)),
                    None => s.exited = true,
                }
                drop(s);
                monitor.changed.notify_waiters();
            }
        });
        *monitor.sampler.lock().unwrap() = Some(task.abort_handle());
        monitor
    }

    pub fn exited(&self) -> bool {
        self.state.lock().unwrap().exited
    }

    pub fn oom_kills(&self) -> u64 {
        self.state.lock().unwrap().oom_kills
    }

    /// Resolves once the engine is gone.
    pub async fn wait_exit(&self) {
        loop {
            let notified = self.changed.notified();
            if self.state.lock().unwrap().exited {
                return;
            }
            notified.await;
        }
    }

    /// Waits until a sample taken after `at` has arrived, so a window that
    /// ends at `at` is bracketed (at most `max`).
    pub async fn wait_sample_after(&self, at: Instant, max: Duration) {
        let deadline = Instant::now() + max;
        loop {
            let notified = self.changed.notified();
            {
                let s = self.state.lock().unwrap();
                if s.exited || s.samples.last().is_some_and(|(t, _)| *t >= at) {
                    return;
                }
            }
            if tokio::time::timeout_at(deadline.into(), notified)
                .await
                .is_err()
            {
                return;
            }
        }
    }

    /// The engine's cost between `from` and `to`: from the last sample at or
    /// before `from` to the first at or after `to`, so a step shorter than
    /// the sampling interval still has two. Rates use the samples' own times.
    pub fn window(&self, from: Instant, to: Instant, cpus: usize) -> Resources {
        let s = self.state.lock().unwrap();
        let inside: Vec<&Sample> = bracket(&s.samples, |(at, _)| *at, from, to)
            .iter()
            .map(|(_, sample)| sample)
            .collect();
        let mut r = Resources {
            samples: inside.len(),
            cpus,
            ..Resources::default()
        };
        let (Some(first), Some(last)) = (inside.first(), inside.last()) else {
            return r;
        };
        let dt = last.t_ms - first.t_ms;
        if dt > 0.0 {
            r.engine_cpu = (last.proc_cpu_ms - first.proc_cpu_ms) / dt;
            r.loadgen_cpu = (last.loadgen_cpu_ms - first.loadgen_cpu_ms) / dt;
            r.cpus_busy = ((last.cpus_busy_ms - first.cpus_busy_ms) / dt).max(0.0);
        }
        let mb = |kb: u64| kb as f64 / 1024.0;
        r.rss_avg_mb = mb(inside.iter().map(|s| s.rss_kb).sum::<u64>() / inside.len() as u64);
        r.rss_max_mb = mb(inside.iter().map(|s| s.rss_kb).max().unwrap_or(0));
        r.hwm_mb = mb(last.hwm_kb);
        r.threads_max = inside.iter().map(|s| s.threads).max().unwrap_or(0);
        r.cgroup_peak_mb = last.cgroup_peak.map(|b| b as f64 / 1024.0 / 1024.0);
        r.machine_mem_mb = mb(last.mem_total_kb);
        r.machine_mem_available_min_mb =
            mb(inside.iter().map(|s| s.mem_available_kb).min().unwrap_or(0));
        r
    }

    pub fn stop(&self) {
        if let Some(task) = self.sampler.lock().unwrap().take() {
            task.abort();
        }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The samples from the last one at or before `from` to the first one at or
/// after `to` (falling back to what is inside when one side is missing).
fn bracket<T>(samples: &[T], at: impl Fn(&T) -> Instant, from: Instant, to: Instant) -> &[T] {
    if samples.is_empty() {
        return samples;
    }
    let first = samples
        .iter()
        .rposition(|x| at(x) <= from)
        .unwrap_or_else(|| {
            samples
                .iter()
                .position(|x| at(x) >= from)
                .unwrap_or(samples.len())
        });
    let last = samples
        .iter()
        .position(|x| at(x) >= to)
        .unwrap_or_else(|| samples.iter().rposition(|x| at(x) <= to).unwrap_or(0));
    if first > last || first >= samples.len() {
        return &samples[..0];
    }
    &samples[first..=last]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_is_bracketed_by_the_samples_around_it() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let samples = [at(0), at(250), at(500), at(750)];
        // A 100 ms window inside one interval still gets two samples.
        assert_eq!(
            bracket(&samples, |t| *t, at(300), at(400)),
            &[at(250), at(500)]
        );
        assert_eq!(
            bracket(&samples, |t| *t, at(250), at(500)),
            &[at(250), at(500)]
        );
        // A window past the last sample keeps what there is.
        assert_eq!(
            bracket(&samples, |t| *t, at(600), at(900)),
            &[at(500), at(750)]
        );
        assert!(bracket(&samples[..0], |t| *t, at(0), at(1)).is_empty());
    }
}
