//! Custom event sources as observable functions.
//!
//! A source that needs setup per observer (a schedule, a watched path) is
//! owner-delivered and takes args; one whose events happen anyway is an auto
//! function, observed with filters.

use iii_sdk::{
    Error, IIIClient, Observe, ObserveHandler, ObserveRejection, Observed, Observer,
    RegisterFunction, Source,
};
use serde_json::json;
use std::pin::Pin;

// ── Example 1: an owner-delivered source with typed args ────────────────

/// What an observer of `example::on-schedule` asks for.
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduleArgs {
    /// ISO 8601 datetime for when to fire (e.g. "2099-01-01T10:00:00Z")
    pub at: String,
    /// Optional timezone (defaults to UTC)
    pub timezone: Option<String>,
    /// Whether to repeat at the same time daily
    pub repeat_daily: Option<bool>,
}

/// One firing of a schedule.
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ScheduleFired {
    /// The scheduled datetime that fired
    pub scheduled_at: String,
    /// Actual firing time
    pub fired_at: String,
    /// How many times this schedule has fired
    pub fire_count: u64,
}

struct Scheduler;

impl ObserveHandler for Scheduler {
    fn on_observer_added<'a>(
        &'a self,
        obs: &Observer,
    ) -> Pin<Box<dyn Future<Output = Result<(), ObserveRejection>> + Send + 'a>> {
        let obs = obs.clone();

        Box::pin(async move {
            let args: ScheduleArgs = obs.args()?;

            if !args.at.ends_with('Z') {
                return Err(ObserveRejection::new(
                    "INVALID_SCHEDULE",
                    format!("`at` must be a UTC timestamp, got {}", args.at),
                ));
            }

            println!("[schedule] {} scheduled at {}", obs.target(), args.at);
            Ok(())
        })
    }
}

// ── Example 2: an owner-delivered source with an enum in its args ───────

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileWatchArgs {
    /// Glob pattern to watch (e.g. "/data/*.csv")
    pub pattern: String,
    /// Watch for these events
    pub events: Vec<FileEvent>,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub enum FileEvent {
    #[serde(rename = "created")]
    Created,
    #[serde(rename = "modified")]
    Modified,
    #[serde(rename = "deleted")]
    Deleted,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct FileChanged {
    pub event: String,
    pub path: String,
    pub size_bytes: Option<u64>,
}

struct FileWatcher;

impl ObserveHandler for FileWatcher {
    fn on_observer_added<'a>(
        &'a self,
        obs: &Observer,
    ) -> Pin<Box<dyn Future<Output = Result<(), ObserveRejection>> + Send + 'a>> {
        let obs = obs.clone();

        Box::pin(async move {
            let args: FileWatchArgs = obs.args()?;
            println!("[file-watch] Watching: {}", args.pattern);
            Ok(())
        })
    }
}

// ── Setup ───────────────────────────────────────────────────────────────

pub fn setup(iii: &IIIClient) {
    // ── Example 1: schedule (owner-delivered, typed args) ───────────
    iii.register_function(
        "example::on-schedule",
        RegisterFunction::new(|_: ScheduleFired| Ok::<_, Error>(()))
            .description("Fires on a one-time or daily schedule. Observe it with args { at }.")
            .observe(
                Observe::owner()
                    .args_format::<ScheduleArgs>()
                    .handler(Scheduler),
            ),
    );
    iii.register_function(
        "example::send_report",
        RegisterFunction::new(|_: Observed<ScheduleFired>| Ok::<_, Error>(json!({ "sent": true }))),
    );
    iii.observe(
        Source::function("example::on-schedule").args(json!({
            "at": "2099-01-01T09:00:00Z",
            "timezone": "America/Sao_Paulo",
            "repeat_daily": true,
        })),
        "example::send_report",
    );

    // ── Example 2: file watch (owner-delivered, enum args) ──────────
    iii.register_function(
        "example::on-file-change",
        RegisterFunction::new(|_: FileChanged| Ok::<_, Error>(()))
            .description(
                "Fires when a watched file changes. Observe it with args { pattern, events }.",
            )
            .observe(
                Observe::owner()
                    .args_format::<FileWatchArgs>()
                    .handler(FileWatcher),
            ),
    );
    iii.register_function(
        "example::process_csv",
        RegisterFunction::new(|event: Observed<FileChanged>| {
            let path = event.input.map(|input| input.path);
            Ok::<_, Error>(json!({ "processed": true, "path": path }))
        }),
    );
    iii.observe(
        Source::function("example::on-file-change")
            .args(json!({ "pattern": "/data/incoming/*.csv", "events": ["created", "modified"] }))
            .filters(json!({ "input.size_bytes": { "$gt": 0 } })),
        "example::process_csv",
    );

    // ── Example 3: an auto function, observed with filters ──────────
    // Every invocation is an event; no args, no hooks.
    iii.register_function(
        "example::custom-event",
        RegisterFunction::new(|input: serde_json::Value| Ok::<_, Error>(input))
            .description("Publishes a custom event"),
    );
    iii.register_function(
        "example::on_custom_event",
        RegisterFunction::new(|event: Observed| Ok::<_, Error>(json!({ "received": event.input }))),
    );
    iii.observe(
        Source::function("example::custom-event")
            .filters(json!({ "input.channel": { "$eq": "notifications" }, "input.priority": { "$eq": "high" } })),
        "example::on_custom_event",
    );
}
