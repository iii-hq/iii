// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii-loadtest export` and `iii-loadtest publish`: what a suite run becomes
//! for the public benchmarks page.
//!
//! `export` turns `results.json` into a [`Summary`]: the run's machine and
//! settings, every step in a compact shape, and per profile the handful of
//! numbers the page leads with ([`Highlights`]), computed here once so the
//! page and the run's own report never disagree. `publish` files a summary
//! into a checkout of the `gh-pages` branch, as `loadtest/runs/<id>.json`, and
//! keeps `loadtest/index.json` (every run, newest first, with its highlights)
//! up to date: the website reads both when it builds.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runner::RunResult;
use crate::scenarios::Step;
use crate::suite::{Profile, Results};

/// Bumped when a field changes meaning; the page refuses a schema it does
/// not know rather than drawing it wrong.
pub const SCHEMA: u32 = 1;

#[derive(clap::Args, Debug, Clone)]
pub struct ExportArgs {
    /// The directory `suite` wrote (holds results.json).
    pub dir: PathBuf,

    /// Write the summary here (default: `<dir>/summary.json`).
    #[arg(long)]
    pub out: Option<PathBuf>,

    /// The version to file the run under (default: the engine's `--version`).
    #[arg(long)]
    pub version: Option<String>,

    /// What ran it, as shown on the page (e.g. `GitHub-hosted, 16 vCPUs`).
    #[arg(long)]
    pub machine: Option<String>,

    /// Where the run can be seen (the CI run's URL).
    #[arg(long)]
    pub url: Option<String>,
}

#[derive(clap::Args, Debug, Clone)]
pub struct PublishArgs {
    /// A summary written by `export`.
    pub summary: PathBuf,

    /// A checkout of the `gh-pages` branch.
    #[arg(long)]
    pub pages: PathBuf,

    /// Runs to keep in the index (the oldest go first).
    #[arg(long, default_value_t = 200)]
    pub keep: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Summary {
    pub schema: u32,
    /// `<version>-<UTC timestamp>`: unique per run, sorts by time within a
    /// version.
    pub id: String,
    pub version: String,
    pub git_rev: Option<String>,
    /// RFC 3339, UTC.
    pub date: String,
    pub machine: Machine,
    pub url: Option<String>,
    pub settings: Value,
    pub profiles: Vec<ProfileInfo>,
    pub highlights: BTreeMap<String, Highlights>,
    pub runs: Vec<SummaryRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Machine {
    pub label: Option<String>,
    pub cpu: String,
    pub cpus: usize,
    pub mem_mb: u64,
    /// vCPUs and MiB the load generator had in each VM.
    pub load_vcpus: u8,
    pub load_ram_mb: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfileInfo {
    pub name: String,
    /// `vm` or `host`.
    pub kind: String,
    /// The engine's CPUs: a VM's vCPUs, or the host CPUs it was pinned to.
    pub cpus: usize,
    /// The engine's memory cap (a VM's); none on the host.
    pub mem_mb: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SummaryRun {
    pub profile: String,
    pub scenario: String,
    pub ok: bool,
    pub error: Option<String>,
    pub oom_killed: bool,
    /// CPUs the engine ran on, and its memory cap when it had one.
    pub engine_cpus: usize,
    pub engine_mem_cap_mb: Option<u64>,
    pub steps: Vec<SummaryStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SummaryStep {
    pub label: String,
    pub params: Value,
    pub unit: String,
    /// Operations per second (`unit` per second); 0 for a step at rest.
    pub rate: f64,
    pub errors: u64,
    pub p50_us: Option<u64>,
    pub p99_us: Option<u64>,
    pub engine_cpu: f64,
    pub cpus_busy: f64,
    pub rss_avg_mb: f64,
    pub rss_max_mb: f64,
    pub loadgen_cpu: f64,
    pub extra: Value,
}

/// The numbers a profile is summed up by. `None` when the scenario did not
/// run, or failed before it measured the number.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Highlights {
    /// `rpc`: the most calls per second at any concurrency (64 B payload).
    pub calls_per_sec: Option<f64>,
    pub calls_p99_us: Option<u64>,
    pub calls_concurrency: Option<u64>,
    /// Engine CPU time per routed call at that point, in microseconds.
    pub cpu_us_per_call: Option<f64>,
    /// `fanout`: the most activations per second.
    pub fanout_activations_per_sec: Option<f64>,
    /// `source`: the highest paced rate delivered in full (≥ 95%)...
    pub sustained_activations_per_sec: Option<f64>,
    /// ...and the first that was not, with the share that arrived.
    pub saturated_at: Option<f64>,
    pub saturated_delivered: Option<f64>,
    /// `filters`: calls per second with the most filtered observers.
    pub filtered_calls_per_sec: Option<f64>,
    pub filtered_observers: Option<u64>,
    /// `match-keys`: calls per second with the most keyed observers, each
    /// call matching one of them.
    pub keyed_calls_per_sec: Option<f64>,
    pub keyed_observers: Option<u64>,
    /// `observers`: memory per held observer, and the most held at rest.
    pub bytes_per_observer: Option<f64>,
    pub observers_held: Option<u64>,
    /// `connections`: memory per idle worker, and the most held.
    pub bytes_per_worker: Option<f64>,
    pub workers_held: Option<u64>,
    /// `local`: in-process calls per second nobody observes, and what the
    /// engine spent meanwhile.
    pub local_calls_per_sec: Option<f64>,
    pub local_engine_cpu: Option<f64>,
    /// The engine at rest with nothing connected.
    pub idle_rss_mb: Option<f64>,
    /// `overload`: the most memory slow observers' queues took, and how many
    /// observers held them.
    pub overload_rss_mb: Option<f64>,
    pub overload_observers: Option<u64>,
    /// Scenarios that did not finish, and whether the cap stopped them.
    pub failed: Vec<Failure>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Failure {
    pub scenario: String,
    pub oom_killed: bool,
    pub error: String,
}

pub fn export(args: &ExportArgs) -> Result<PathBuf> {
    let path = args.dir.join("results.json");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let results: Results = serde_json::from_str(&text)?;
    let summary = summarize(&results, args);
    let out = args
        .out
        .clone()
        .unwrap_or_else(|| args.dir.join("summary.json"));
    std::fs::write(&out, serde_json::to_vec_pretty(&summary)?)?;
    Ok(out)
}

pub fn summarize(results: &Results, args: &ExportArgs) -> Summary {
    let m = &results.meta;
    let version = args.version.clone().unwrap_or_else(|| {
        m.engine_version
            .trim()
            .trim_start_matches("iii ")
            .to_string()
    });
    let date = rfc3339(m.started_unix);
    let mut profiles: Vec<ProfileInfo> = Vec::new();
    for run in &results.runs {
        if profiles.iter().any(|p| p.name == run.profile) {
            continue;
        }
        profiles.push(match Profile::parse(&run.profile) {
            Ok(Profile::Vm { vcpus, ram_mib }) => ProfileInfo {
                name: run.profile.clone(),
                kind: "vm".into(),
                cpus: usize::from(vcpus),
                mem_mb: Some(ram_mib),
            },
            _ => ProfileInfo {
                name: run.profile.clone(),
                kind: "host".into(),
                cpus: if run.engine_cpus > 0 {
                    run.engine_cpus
                } else {
                    m.engine_host_cpus.len()
                },
                mem_mb: None,
            },
        });
    }
    let highlights = profiles
        .iter()
        .map(|p| (p.name.clone(), highlights(&results.runs, &p.name)))
        .collect();
    Summary {
        schema: SCHEMA,
        id: format!("{version}-{}", compact_date(&date)),
        version,
        git_rev: m.git_rev.clone(),
        date,
        machine: Machine {
            label: args.machine.clone(),
            cpu: m.host_cpu.clone(),
            cpus: m.host_cpus,
            mem_mb: m.host_mem_mb,
            load_vcpus: m.load_vcpus,
            load_ram_mb: m.load_ram_mb,
        },
        url: args.url.clone(),
        settings: serde_json::to_value(&m.settings).unwrap_or(Value::Null),
        profiles,
        highlights,
        runs: results.runs.iter().map(summary_run).collect(),
    }
}

fn summary_run(run: &RunResult) -> SummaryRun {
    SummaryRun {
        profile: run.profile.clone(),
        scenario: run.scenario.clone(),
        ok: run.ok,
        error: run.error.clone(),
        oom_killed: run.oom_killed,
        engine_cpus: run.engine_cpus,
        engine_mem_cap_mb: run.engine_mem_cap_mb,
        steps: run.steps.iter().map(summary_step).collect(),
    }
}

fn summary_step(step: &Step) -> SummaryStep {
    let r = &step.resources;
    let round = |v: f64, places: i32| {
        let k = 10f64.powi(places);
        (v * k).round() / k
    };
    SummaryStep {
        label: step.label.clone(),
        params: step.params.clone(),
        unit: step.unit.clone(),
        rate: if step.unit == "idle" {
            0.0
        } else {
            round(step.ops_per_sec, 1)
        },
        errors: step.errors,
        p50_us: step.latency.as_ref().map(|l| l.p50_us),
        p99_us: step.latency.as_ref().map(|l| l.p99_us),
        engine_cpu: round(r.engine_cpu, 3),
        cpus_busy: round(r.cpus_busy, 3),
        rss_avg_mb: round(r.rss_avg_mb, 1),
        rss_max_mb: round(r.rss_max_mb, 1),
        loadgen_cpu: round(r.loadgen_cpu, 2),
        extra: step.extra.clone(),
    }
}

/// The steps a scenario measured on a profile (not the one its engine died
/// in, if it did).
fn steps<'a>(runs: &'a [RunResult], profile: &str, scenario: &str) -> Vec<&'a Step> {
    runs.iter()
        .filter(|r| r.profile == profile && r.scenario == scenario)
        .flat_map(|r| r.steps.iter())
        .filter(|s| s.unit != "failed")
        .collect()
}

fn extra_f64(step: &Step, key: &str) -> Option<f64> {
    step.extra.get(key).and_then(Value::as_f64)
}

fn param_u64(step: &Step, key: &str) -> Option<u64> {
    step.params.get(key).and_then(Value::as_u64)
}

pub fn highlights(runs: &[RunResult], profile: &str) -> Highlights {
    let mut h = Highlights::default();

    let best_rpc = steps(runs, profile, "rpc")
        .into_iter()
        .filter(|s| param_u64(s, "payload_bytes") == Some(64))
        .max_by(|a, b| a.ops_per_sec.total_cmp(&b.ops_per_sec));
    if let Some(s) = best_rpc {
        h.calls_per_sec = Some(s.ops_per_sec);
        h.calls_p99_us = s.latency.as_ref().map(|l| l.p99_us);
        h.calls_concurrency = param_u64(s, "concurrency");
        if s.ops_per_sec > 0.0 {
            h.cpu_us_per_call = Some(s.resources.engine_cpu / s.ops_per_sec * 1e6);
        }
    }

    h.fanout_activations_per_sec = steps(runs, profile, "fanout")
        .into_iter()
        .filter_map(|s| extra_f64(s, "activations_per_sec"))
        .reduce(f64::max);

    let paced: Vec<(f64, f64)> = steps(runs, profile, "source")
        .into_iter()
        .filter_map(|s| {
            Some((
                extra_f64(s, "offered_per_sec")?,
                extra_f64(s, "delivery_ratio")?,
            ))
        })
        .collect();
    h.sustained_activations_per_sec = paced
        .iter()
        .filter(|(_, ratio)| *ratio >= 0.95)
        .map(|(rate, _)| *rate)
        .reduce(f64::max);
    if let Some((rate, ratio)) = paced.iter().find(|(_, ratio)| *ratio < 0.95) {
        h.saturated_at = Some(*rate);
        h.saturated_delivered = Some(*ratio);
    }

    if let Some(s) = steps(runs, profile, "filters").into_iter().last() {
        h.filtered_calls_per_sec = Some(s.ops_per_sec);
        h.filtered_observers = param_u64(s, "observers");
    }

    if let Some(s) = steps(runs, profile, "match-keys")
        .into_iter()
        .rev()
        .find(|s| param_u64(s, "matching") == Some(1))
    {
        h.keyed_calls_per_sec = Some(s.ops_per_sec);
        h.keyed_observers = param_u64(s, "observers");
    }

    let at_rest = |scenario: &str, key: &str, count: &str| {
        steps(runs, profile, scenario)
            .into_iter()
            .filter(|s| extra_f64(s, key).is_some())
            .max_by_key(|s| param_u64(s, count).unwrap_or(0))
            .map(|s| (extra_f64(s, key), param_u64(s, count)))
    };
    if let Some((bytes, held)) = at_rest("observers", "rss_bytes_per_observer", "observers") {
        h.bytes_per_observer = bytes;
        h.observers_held = held;
    }
    if let Some((bytes, held)) = at_rest("connections", "rss_bytes_per_worker", "workers") {
        h.bytes_per_worker = bytes;
        h.workers_held = held;
    }

    if let Some(s) = steps(runs, profile, "local")
        .into_iter()
        .find(|s| s.label == "unobserved")
    {
        h.local_calls_per_sec = Some(s.ops_per_sec);
        h.local_engine_cpu = Some(s.resources.engine_cpu);
    }

    h.idle_rss_mb = ["observers", "connections"].iter().find_map(|scenario| {
        steps(runs, profile, scenario)
            .into_iter()
            .find(|s| {
                s.unit == "idle" && param_u64(s, "observers").or(param_u64(s, "workers")) == Some(0)
            })
            .map(|s| s.resources.rss_avg_mb)
    });

    if let Some(s) = steps(runs, profile, "overload")
        .into_iter()
        .filter(|s| s.unit != "idle")
        .max_by(|a, b| a.resources.rss_max_mb.total_cmp(&b.resources.rss_max_mb))
    {
        h.overload_rss_mb = Some(s.resources.rss_max_mb);
        h.overload_observers = param_u64(s, "observers");
    }

    h.failed = runs
        .iter()
        .filter(|r| r.profile == profile && !r.ok)
        .map(|r| Failure {
            scenario: r.scenario.clone(),
            oom_killed: r.oom_killed,
            error: r
                .error
                .clone()
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect(),
        })
        .collect();
    h
}

/// An entry of `loadtest/index.json`: enough to list runs and draw trends
/// without opening each one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexEntry {
    pub id: String,
    pub version: String,
    pub git_rev: Option<String>,
    pub date: String,
    pub machine: Machine,
    pub url: Option<String>,
    /// Relative to `loadtest/`.
    pub path: String,
    pub profiles: Vec<ProfileInfo>,
    pub highlights: BTreeMap<String, Highlights>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Index {
    pub schema: u32,
    pub runs: Vec<IndexEntry>,
}

pub fn publish(args: &PublishArgs) -> Result<()> {
    let text = std::fs::read_to_string(&args.summary)
        .with_context(|| format!("read {}", args.summary.display()))?;
    let summary: Summary = serde_json::from_str(&text)?;
    let dir = args.pages.join("loadtest");
    std::fs::create_dir_all(dir.join("runs"))?;
    let path = format!("runs/{}.json", summary.id);
    std::fs::write(dir.join(&path), text)?;

    let index_path = dir.join("index.json");
    let mut index: Index = match std::fs::read_to_string(&index_path) {
        Ok(text) => serde_json::from_str(&text).context("loadtest/index.json")?,
        Err(_) => Index::default(),
    };
    let removed = add_to_index(&mut index, &summary, path, args.keep);
    for old in removed {
        let _ = std::fs::remove_file(dir.join(old));
    }
    std::fs::write(&index_path, serde_json::to_vec_pretty(&index)?)?;
    Ok(())
}

/// Puts `summary` first (replacing a run with the same id), keeps `keep`
/// runs, and returns the paths of the runs dropped.
fn add_to_index(index: &mut Index, summary: &Summary, path: String, keep: usize) -> Vec<String> {
    index.schema = SCHEMA;
    index.runs.retain(|r| r.id != summary.id);
    index.runs.insert(
        0,
        IndexEntry {
            id: summary.id.clone(),
            version: summary.version.clone(),
            git_rev: summary.git_rev.clone(),
            date: summary.date.clone(),
            machine: summary.machine.clone(),
            url: summary.url.clone(),
            path,
            profiles: summary.profiles.clone(),
            highlights: summary.highlights.clone(),
        },
    );
    index.runs.sort_by(|a, b| b.date.cmp(&a.date));
    if index.runs.len() > keep {
        index
            .runs
            .split_off(keep)
            .into_iter()
            .map(|r| r.path)
            .collect()
    } else {
        Vec::new()
    }
}

/// Seconds since the epoch as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn rfc3339(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// `2026-10-19T18:04:05Z` → `20261019T1804`.
fn compact_date(date: &str) -> String {
    date.chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T')
        .take(13)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::Resources;
    use crate::runner::Settings;
    use crate::stats::Latency;
    use crate::suite::Meta;
    use serde_json::json;

    fn step(label: &str, unit: &str, rate: f64, params: Value, extra: Value, rss: f64) -> Step {
        Step {
            label: label.into(),
            params,
            secs: 10.0,
            ops: (rate * 10.0) as u64,
            unit: unit.into(),
            errors: 0,
            ops_per_sec: rate,
            latency: Some(Latency {
                p99_us: 900,
                ..Latency::default()
            }),
            first_error: None,
            extra,
            resources: Resources {
                engine_cpu: 1.8,
                rss_avg_mb: rss,
                rss_max_mb: rss,
                ..Resources::default()
            },
        }
    }

    fn run(profile: &str, scenario: &str, steps: Vec<Step>) -> RunResult {
        RunResult {
            profile: profile.into(),
            scenario: scenario.into(),
            ok: true,
            error: None,
            engine_mem_cap_mb: None,
            engine_cpus: 2,
            oom_killed: false,
            steps,
        }
    }

    fn results() -> Results {
        let runs = vec![
            run(
                "2x1024",
                "rpc",
                vec![
                    step(
                        "c 32",
                        "calls",
                        70_000.0,
                        json!({"concurrency": 32, "payload_bytes": 64}),
                        json!({}),
                        43.0,
                    ),
                    step(
                        "c 128",
                        "calls",
                        90_000.0,
                        json!({"concurrency": 128, "payload_bytes": 64}),
                        json!({}),
                        43.0,
                    ),
                    step(
                        "16 KiB",
                        "calls",
                        95_000.0,
                        json!({"concurrency": 32, "payload_bytes": 16384}),
                        json!({}),
                        48.0,
                    ),
                ],
            ),
            run(
                "2x1024",
                "source",
                vec![
                    step(
                        "1k",
                        "calls",
                        1000.0,
                        json!({}),
                        json!({"offered_per_sec": 1000, "delivery_ratio": 1.0}),
                        60.0,
                    ),
                    step(
                        "50k",
                        "calls",
                        50_000.0,
                        json!({}),
                        json!({"offered_per_sec": 50000, "delivery_ratio": 0.99}),
                        60.0,
                    ),
                    step(
                        "100k",
                        "calls",
                        100_000.0,
                        json!({}),
                        json!({"offered_per_sec": 100000, "delivery_ratio": 0.64}),
                        60.0,
                    ),
                ],
            ),
            run(
                "2x1024",
                "observers",
                vec![
                    step(
                        "0 (at rest)",
                        "idle",
                        0.0,
                        json!({"observers": 0}),
                        json!({}),
                        41.0,
                    ),
                    step(
                        "1000 (at rest)",
                        "idle",
                        0.0,
                        json!({"observers": 1000}),
                        json!({"rss_bytes_per_observer": 14000.0}),
                        50.0,
                    ),
                    step(
                        "4000 (at rest)",
                        "idle",
                        0.0,
                        json!({"observers": 4000}),
                        json!({"rss_bytes_per_observer": 10500.0}),
                        75.0,
                    ),
                ],
            ),
            RunResult {
                ok: false,
                oom_killed: true,
                error: Some("the engine was OOM-killed at its 1024 MiB cap".into()),
                ..run("2x1024", "overload", vec![])
            },
        ];
        Results {
            meta: Meta {
                started_unix: 1_792_440_245,
                engine: "/x/iii".into(),
                engine_version: "0.25.0".into(),
                git_rev: Some("73a846b1c".into()),
                host_cpu: "AMD EPYC".into(),
                host_cpus: 16,
                host_mem_mb: 64_000,
                engine_host_cpus: vec![],
                load_host_cpus: vec![],
                load_vcpus: 6,
                load_ram_mb: 2048,
                settings: Settings {
                    step_secs: 10,
                    warmup_secs: 2,
                    quick: false,
                    observers: vec![],
                    engine_log_level: "info".into(),
                    observability: "default".into(),
                },
            },
            runs,
        }
    }

    fn args() -> ExportArgs {
        ExportArgs {
            dir: "/x".into(),
            out: None,
            version: None,
            machine: Some("GitHub-hosted, 16 vCPUs".into()),
            url: None,
        }
    }

    #[test]
    fn highlights_take_the_number_each_scenario_is_about() {
        let summary = summarize(&results(), &args());
        let h = &summary.highlights["2x1024"];
        // The 16 KiB step is not the routing number.
        assert_eq!(h.calls_per_sec, Some(90_000.0));
        assert_eq!(h.calls_concurrency, Some(128));
        assert_eq!(h.calls_p99_us, Some(900));
        assert!((h.cpu_us_per_call.unwrap() - 20.0).abs() < 1e-9);
        assert_eq!(h.sustained_activations_per_sec, Some(50_000.0));
        assert_eq!(h.saturated_at, Some(100_000.0));
        assert_eq!(h.saturated_delivered, Some(0.64));
        assert_eq!(h.bytes_per_observer, Some(10_500.0));
        assert_eq!(h.observers_held, Some(4000));
        assert_eq!(h.idle_rss_mb, Some(41.0));
        assert_eq!(h.failed.len(), 1);
        assert!(h.failed[0].oom_killed);
        assert_eq!(h.local_calls_per_sec, None);
    }

    #[test]
    fn a_summary_carries_the_run_and_its_profiles() {
        let summary = summarize(&results(), &args());
        assert_eq!(summary.schema, SCHEMA);
        assert_eq!(summary.version, "0.25.0");
        assert_eq!(summary.date, "2026-10-19T20:04:05Z");
        assert_eq!(summary.id, "0.25.0-20261019T2004");
        assert_eq!(
            summary.profiles,
            vec![ProfileInfo {
                name: "2x1024".into(),
                kind: "vm".into(),
                cpus: 2,
                mem_mb: Some(1024)
            }]
        );
        assert_eq!(summary.runs.len(), 4);
        assert_eq!(summary.runs[0].steps[0].rate, 70_000.0);
        assert_eq!(
            summary.machine.label.as_deref(),
            Some("GitHub-hosted, 16 vCPUs")
        );
    }

    #[test]
    fn the_index_keeps_the_newest_runs_first_and_drops_the_oldest() {
        let mut summary = summarize(&results(), &args());
        let mut index = Index::default();
        for (i, date) in [
            "2026-10-01T00:00:00Z",
            "2026-10-03T00:00:00Z",
            "2026-10-02T00:00:00Z",
        ]
        .iter()
        .enumerate()
        {
            summary.id = format!("run-{i}");
            summary.date = date.to_string();
            let dropped = add_to_index(&mut index, &summary, format!("runs/run-{i}.json"), 2);
            if i == 2 {
                assert_eq!(dropped, vec!["runs/run-0.json".to_string()]);
            }
        }
        let ids: Vec<&str> = index.runs.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["run-1", "run-2"]);
        // Publishing a run again replaces it.
        summary.id = "run-1".into();
        add_to_index(&mut index, &summary, "runs/run-1.json".into(), 2);
        assert_eq!(index.runs.len(), 2);
    }

    #[test]
    fn dates_are_utc_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_792_440_245), "2026-10-19T20:04:05Z");
    }
}
