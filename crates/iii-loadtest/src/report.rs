// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `report.md` from `results.json`: a capacity summary per profile, then
//! every step of every scenario.

use std::fmt::Write;

use serde_json::Value;

use crate::runner::RunResult;
use crate::scenarios::{self, Step, fmt_us};
use crate::suite::Results;

pub fn render(results: &Results) -> String {
    let m = &results.meta;
    let st = &m.settings;
    let mut out = String::new();
    let _ = writeln!(out, "# iii engine load test\n");
    let _ = writeln!(
        out,
        "- **Engine:** {} (`{}`){}",
        m.engine_version,
        m.engine,
        m.git_rev
            .as_deref()
            .map(|r| format!(", git `{r}`"))
            .unwrap_or_default()
    );
    let _ = writeln!(
        out,
        "- **Host:** {} ({} CPUs, {} MiB). The engine (its vCPUs, or the `host` engine) runs on host \
         CPUs {:?}, the load generator on {:?}: never the same physical core.",
        m.host_cpu, m.host_cpus, m.host_mem_mb, m.engine_host_cpus, m.load_host_cpus
    );
    let _ = writeln!(
        out,
        "- **Profiles:** `NxM` is a libkrun microVM (KVM) where the engine has N vCPUs and a \
         cgroup cap of M MiB (OOM-killed past it); the load generator runs in the same VM on {} \
         vCPUs of its own with {} MiB, over the guest's loopback. `host` is the engine as a plain \
         process, uncapped: the reference without a VM.",
        m.load_vcpus, m.load_ram_mb
    );
    let _ = writeln!(
        out,
        "- **Steps:** {} s of load each{}. Engine log level `{}`, observability `{}`{}.{}",
        if st.quick {
            st.step_secs.min(3)
        } else {
            st.step_secs
        },
        if st.quick {
            " (quick run: fewer, shorter steps)"
        } else {
            ""
        },
        st.engine_log_level,
        st.observability,
        if st.observability == "default" {
            " (as shipped: every span recorded, traces kept in memory and on disk)"
        } else {
            " (turned off)"
        },
        if st.observers.is_empty() {
            String::new()
        } else {
            format!(
                " Engine `observers:` overrides: {}.",
                st.observers.join(", ")
            )
        }
    );
    let _ = writeln!(
        out,
        "- **Columns:** CPU in cores (1.00 = one CPU busy for the whole step). *engine* is the \
         engine process; *CPUs busy* is how busy the engine's CPUs were, kernel work on them \
         included (network softirqs), out of the N it has. **RSS** is the engine's resident memory.\n"
    );

    summary(&mut out, &results.runs);

    for name in scenarios::ALL {
        let runs: Vec<&RunResult> = results
            .runs
            .iter()
            .filter(|r| r.scenario == *name)
            .collect();
        if runs.is_empty() {
            continue;
        }
        let _ = writeln!(out, "## `{name}`: {}\n", scenarios::describe(name));
        let _ = writeln!(
            out,
            "| profile | step | rate | p50 | p99 | errors | engine CPU | CPUs busy | RSS max (MiB) | notes |"
        );
        let _ = writeln!(out, "|---|---|---:|---:|---:|---:|---:|---:|---:|---|");
        for run in runs {
            for step in &run.steps {
                let r = &step.resources;
                let (p50, p99) = step
                    .latency
                    .as_ref()
                    .map(|l| (fmt_us(l.p50_us), fmt_us(l.p99_us)))
                    .unwrap_or_default();
                let rate = if step.unit == "idle" {
                    "–".to_string()
                } else if step.unit == "failed" {
                    "**the engine died here**".to_string()
                } else {
                    format!("{} {}/s", fmt_rate(step.ops_per_sec), step.unit)
                };
                let _ = writeln!(
                    out,
                    "| {} | {} | {} | {} | {} | {} | {:.2} | {:.2} / {} | {:.0} | {} |",
                    run.profile,
                    step.label,
                    rate,
                    p50,
                    p99,
                    step.errors,
                    r.engine_cpu,
                    r.cpus_busy,
                    r.cpus,
                    r.rss_max_mb,
                    notes(step),
                );
            }
            if let Some(e) = &run.error {
                let e: String = e
                    .replace('|', "/")
                    .replace('\n', " ")
                    .chars()
                    .take(300)
                    .collect();
                let _ = writeln!(out, "| {} | **failed** | | | | | | | | {e} |", run.profile);
            }
        }
        let _ = writeln!(out);
    }
    out
}

/// One line per profile with the number each scenario is about.
fn summary(out: &mut String, runs: &[RunResult]) {
    let mut profiles: Vec<&str> = Vec::new();
    for r in runs {
        if !profiles.contains(&r.profile.as_str()) {
            profiles.push(&r.profile);
        }
    }
    let _ = writeln!(out, "## Capacity at a glance\n");
    let _ = writeln!(
        out,
        "| profile | rpc: max calls/s | fanout: activations/s | filters: calls/s at the most observers | \
         observers: bytes each | emit: sustained emits/s | local: calls/s unobserved | connections: bytes each |"
    );
    let _ = writeln!(out, "|---|---:|---:|---:|---:|---:|---:|---:|");
    for p in profiles {
        let steps = |scenario: &str| -> Vec<&Step> {
            runs.iter()
                .filter(|r| r.profile == p && r.scenario == scenario)
                .flat_map(|r| r.steps.iter())
                .collect()
        };
        let best = |steps: Vec<&Step>| -> String {
            steps
                .into_iter()
                .filter(|s| s.unit != "idle")
                .max_by(|a, b| a.ops_per_sec.total_cmp(&b.ops_per_sec))
                .map(|s| {
                    let p99 = s
                        .latency
                        .as_ref()
                        .map(|l| format!(", p99 {}", fmt_us(l.p99_us)))
                        .unwrap_or_default();
                    format!(
                        "{} ({}{p99}, engine {:.2} cpu)",
                        fmt_rate(s.ops_per_sec),
                        s.label,
                        s.resources.engine_cpu
                    )
                })
                .unwrap_or_else(|| "–".into())
        };
        let extra_max = |steps: Vec<&Step>, key: &str| -> String {
            steps
                .into_iter()
                .filter_map(|s| s.extra.get(key).and_then(Value::as_f64).map(|v| (v, s)))
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(v, s)| format!("{} ({})", fmt_rate(v), s.label))
                .unwrap_or_else(|| "–".into())
        };
        let last_extra = |steps: Vec<&Step>, key: &str| -> String {
            steps
                .into_iter()
                .rev()
                .find_map(|s| s.extra.get(key).and_then(Value::as_f64).map(|v| (v, s)))
                .map(|(v, s)| {
                    format!(
                        "{} ({})",
                        fmt_bytes(v),
                        s.label.trim_end_matches(" (at rest)")
                    )
                })
                .unwrap_or_else(|| "–".into())
        };
        let filters_last = steps("filters")
            .into_iter()
            .last()
            .map(|s| format!("{} ({})", fmt_rate(s.ops_per_sec), s.label))
            .unwrap_or_else(|| "–".into());
        let local_unobserved = steps("local")
            .into_iter()
            .find(|s| s.label == "unobserved")
            .map(|s| {
                format!(
                    "{} (engine {:.2} cpu)",
                    fmt_rate(s.ops_per_sec),
                    s.resources.engine_cpu
                )
            })
            .unwrap_or_else(|| "–".into());
        let _ = writeln!(
            out,
            "| {p} | {} | {} | {} | {} | {} | {} | {} |",
            best(steps("rpc")),
            extra_max(steps("fanout"), "activations_per_sec"),
            filters_last,
            last_extra(steps("observers"), "rss_bytes_per_observer"),
            sustained(steps("emit")),
            local_unobserved,
            last_extra(steps("connections"), "rss_bytes_per_worker"),
        );
    }
    let _ = writeln!(out);
}

/// The highest paced rate delivered at least 95%, and the first that was not.
fn sustained(steps: Vec<&Step>) -> String {
    let paced: Vec<(&Step, f64, f64)> = steps
        .into_iter()
        .filter_map(|s| {
            let rate = s.extra.get("offered_per_sec")?.as_f64()?;
            let ratio = s.extra.get("delivery_ratio")?.as_f64()?;
            Some((s, rate, ratio))
        })
        .collect();
    let best = paced
        .iter()
        .filter(|(_, _, r)| *r >= 0.95)
        .map(|(_, rate, _)| *rate)
        .reduce(f64::max);
    let broke = paced.iter().find(|(_, _, r)| *r < 0.95);
    match (best, broke) {
        (Some(b), Some((_, rate, ratio))) => format!(
            "{} (at {}: {:.0}% delivered)",
            fmt_rate(b),
            fmt_rate(*rate),
            ratio * 100.0
        ),
        (Some(b), None) => format!("≥ {} (highest tried)", fmt_rate(b)),
        (None, Some((_, rate, ratio))) => {
            format!("< {} ({:.0}% delivered)", fmt_rate(*rate), ratio * 100.0)
        }
        (None, None) => "–".into(),
    }
}

fn notes(step: &Step) -> String {
    let e = &step.extra;
    let mut parts = Vec::new();
    if let Some(v) = e.get("activations_per_sec").and_then(Value::as_f64) {
        parts.push(format!("{} activations/s", fmt_rate(v)));
    }
    if let Some(v) = e.pointer("/engine/fires").and_then(Value::as_u64)
        && v > 0
    {
        parts.push(format!("fires {v}"));
    }
    match (
        e.get("activations_delivered").and_then(Value::as_u64),
        e.get("activations_expected").and_then(Value::as_u64),
    ) {
        (Some(d), Some(x)) if x > 0 => parts.push(format!("delivered {d}/{x}")),
        (Some(d), _) => parts.push(format!("delivered {d}")),
        _ => {}
    }
    if let Some(r) = e.get("delivery_ratio").and_then(Value::as_f64) {
        parts.push(format!("{:.1}% delivered", r * 100.0));
    }
    if let Some(dropped) = e.pointer("/engine/dropped").and_then(Value::as_object)
        && !dropped.is_empty()
    {
        let list: Vec<String> = dropped.iter().map(|(k, v)| format!("{k} {v}")).collect();
        parts.push(format!("dropped: {}", list.join(", ")));
    }
    if let Some(failed) = e
        .pointer("/engine/activation_failures")
        .and_then(Value::as_object)
        && !failed.is_empty()
    {
        let list: Vec<String> = failed.iter().map(|(k, v)| format!("{k} {v}")).collect();
        parts.push(format!("failed: {}", list.join(", ")));
    }
    if let Some(v) = e.get("emits_not_accepted").and_then(Value::as_u64)
        && v > 0
    {
        parts.push(format!("{v} emits not accepted"));
    }
    if let Some(v) = e.get("observers_created_per_sec").and_then(Value::as_f64) {
        parts.push(format!("observers created at {}/s", fmt_rate(v)));
    }
    if let Some(v) = e.get("rss_bytes_per_observer").and_then(Value::as_f64) {
        parts.push(format!("{} per observer", fmt_bytes(v)));
    }
    if let Some(p) = e.get("phases_p50_us").and_then(Value::as_object) {
        let us = |k: &str| {
            p.get(k)
                .and_then(Value::as_u64)
                .map(fmt_us)
                .unwrap_or_default()
        };
        parts.push(format!(
            "median TCP {} + upgrade {} + workerregistered {}",
            us("tcp"),
            us("handshake"),
            us("registered")
        ));
    }
    if let Some(v) = e.get("rss_bytes_per_worker").and_then(Value::as_f64) {
        parts.push(format!("{} per worker", fmt_bytes(v)));
    }
    if let Some(err) = &step.first_error {
        parts.push(format!(
            "first error: {}",
            err.replace('|', "/").chars().take(120).collect::<String>()
        ));
    }
    parts.join("; ")
}

pub fn fmt_rate(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.2}M", v / 1_000_000.0)
    } else if v >= 10_000.0 {
        format!("{:.0}k", v / 1000.0)
    } else if v >= 1000.0 {
        format!("{:.1}k", v / 1000.0)
    } else {
        format!("{v:.0}")
    }
}

pub fn fmt_bytes(v: f64) -> String {
    if v.abs() >= 1024.0 * 1024.0 {
        format!("{:.1} MiB", v / 1024.0 / 1024.0)
    } else if v.abs() >= 1024.0 {
        format!("{:.1} KiB", v / 1024.0)
    } else {
        format!("{v:.0} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_and_sizes_read_well() {
        assert_eq!(fmt_rate(950.0), "950");
        assert_eq!(fmt_rate(4_200.0), "4.2k");
        assert_eq!(fmt_rate(42_000.0), "42k");
        assert_eq!(fmt_rate(1_500_000.0), "1.50M");
        assert_eq!(fmt_bytes(512.0), "512 B");
        assert_eq!(fmt_bytes(2048.0), "2.0 KiB");
        assert_eq!(fmt_bytes(3.0 * 1024.0 * 1024.0), "3.0 MiB");
    }
}
