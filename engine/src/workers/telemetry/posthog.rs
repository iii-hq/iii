// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

const POSTHOG_DEFAULT_HOST: &str = "https://us.i.posthog.com";
const MAX_RETRIES: u32 = 3;

pub const POSTHOG_PROJECT_API_KEY: &str = "phc_mmRHNXK6hkykVuxVp3JPn7R7sbo3ckSpEZLUKjofCWn6";

/// An address-shaped run of text, redacted wherever it appears in an error.
///
/// One `@`, a local part, and a dotted domain ending in letters. Deliberately
/// broader than the address the operator typed: the point is that no error
/// string can carry an address, whether or not this build knows which one.
/// It also catches `user@host` forms that are not addresses, which is the
/// price of not having to be right about which is which.
static EMAIL_IN_ERROR: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
    regex::Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}")
        .expect("the address pattern is a literal and compiles")
});

/// Strip `/Users/<name>/`, `/home/<name>/`, and Windows `\Users\<name>\` /
/// `\home\<name>\` prefixes from error strings, redact anything
/// address-shaped, and cap the length so we never ship unbounded backtraces.
///
/// Applied at the send layer: every event goes through
/// [`build_posthog_event`], so an error is scrubbed regardless of which
/// subsystem produced it.
///
/// The address redaction is defence in depth. Nothing puts an address in an
/// `error` today, and the one worker that holds one never logs it. This is
/// what keeps that true when someone later writes `could not add {email}` in
/// a catch block.
pub fn sanitize_error(error: &str) -> String {
    const MAX_LEN: usize = 256;
    let mut out = String::with_capacity(error.len().min(MAX_LEN));
    let mut chars = error.chars().peekable();
    let mut buf = String::new();
    while let Some(c) = chars.next() {
        buf.push(c);
        if buf.ends_with("/Users/")
            || buf.ends_with("/home/")
            || buf.ends_with("\\Users\\")
            || buf.ends_with("\\home\\")
        {
            out.push_str(&buf);
            buf.clear();
            // Skip the username segment up to the next path separator
            // (forward or back slash) or whitespace.
            while let Some(&peek) = chars.peek() {
                if peek == '/' || peek == '\\' || peek.is_whitespace() {
                    break;
                }
                chars.next();
            }
            out.push_str("<redacted>");
        }
    }
    out.push_str(&buf);
    // After the path scrub, so a redacted home directory cannot leave behind
    // something that only now looks like an address. Before the length cap, so
    // an address near the end of a long error is redacted rather than
    // truncated into something still readable.
    let out = EMAIL_IN_ERROR.replace_all(&out, "<redacted>").into_owned();
    if out.chars().count() > MAX_LEN {
        let truncated: String = out.chars().take(MAX_LEN).collect();
        format!("{truncated}…")
    } else {
        out
    }
}

/// Recursively walk a JSON value and apply [`sanitize_error`] to any string
/// stored under a key named `"error"`. This way the redaction applies to
/// any event whose `event_properties` carry an `error` field,
/// without each call site having to remember to sanitize.
fn sanitize_event_properties(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k == "error" {
                    if let serde_json::Value::String(s) = v {
                        *s = sanitize_error(s);
                    }
                } else {
                    sanitize_event_properties(v);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                sanitize_event_properties(v);
            }
        }
        _ => {}
    }
}

/// One product event, before it is shaped for PostHog.
#[derive(Debug, Clone, Serialize)]
pub struct ProductEvent {
    pub device_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    pub event_type: String,
    pub event_properties: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_properties: Option<serde_json::Value>,
    pub platform: String,
    pub os_name: String,
    pub app_version: String,
    pub time: i64,
    pub insert_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
}

#[derive(Serialize)]
struct PostHogPayload {
    api_key: String,
    historical_migration: bool,
    batch: Vec<PostHogEvent>,
}

#[derive(Serialize)]
struct PostHogEvent {
    event: String,
    properties: serde_json::Value,
    timestamp: String,
    uuid: Option<String>,
}

/// Client for sending anonymous product analytics to PostHog.
pub struct PostHogClient {
    api_key: String,
    host: String,
    client: reqwest::Client,
}

pub fn posthog_user_mode(event_type: &str) -> &'static str {
    match event_type {
        "heartbeat" | "engine_stopped" => "using",
        _ => "building",
    }
}

fn posthog_batch_url(host: &str) -> String {
    format!("{}/batch/", host.trim_end_matches('/'))
}

fn posthog_timestamp_millis(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339()
}

fn should_skip_posthog_user_property(
    key: &str,
    properties: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    match key {
        // app_version is the canonical PostHog field; iii_version is an older alias.
        "iii_version" => properties.contains_key("app_version"),
        _ => false,
    }
}

fn should_skip_posthog_event_property(
    key: &str,
    properties: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    match key {
        "version" => {
            properties.contains_key("app_version") || properties.contains_key("iii_version")
        }
        "function_names" => true, // deprecated alias of `functions`
        _ => false,
    }
}

/// The one event that carries an address the user gave us.
pub const IDENTIFY_EVENT: &str = "user_identified";

/// The person property the address is written to.
const EMAIL_PERSON_PROPERTY: &str = "email";

/// Move the address out of the event properties and into a PostHog `$set`, so
/// it lands on the person rather than on the one event.
///
/// `$set` overwrites, which is what a corrected address needs. The person
/// stays keyed by `device_id`: nothing is aliased and nothing is merged.
///
/// TODO: Change this to PostHog's true `$identify` when iii cloud is available
/// and can provide stable IDs. Until then an address is a property of the
/// machine's person, and one human on two machines is two persons.
fn set_person_email(
    event_type: &str,
    properties: &mut serde_json::Map<String, serde_json::Value>,
) -> bool {
    if event_type != IDENTIFY_EVENT {
        return false;
    }
    let Some(email) = properties.remove(EMAIL_PERSON_PROPERTY) else {
        return false;
    };
    properties.insert(
        "$set".into(),
        serde_json::json!({ EMAIL_PERSON_PROPERTY: email }),
    );
    true
}

/// Uptime a session must pass for its person to be flagged as long-running.
const LONG_SESSION_UPTIME_SECS: u64 = 200;

/// The person property a passing heartbeat sets.
const LONG_SESSION_PERSON_PROPERTY: &str = "uptime_is_greater_than_200_secs";

/// Whether this process has already written the long-session flag.
///
/// The property never changes once written, so one write per process is
/// enough. Writing it again costs an identified event for nothing, and a
/// long-running engine reports a passing uptime on every heartbeat. Restarts
/// write it again, which is one event per run.
static LONG_SESSION_FLAG_WRITTEN: AtomicBool = AtomicBool::new(false);

/// Whether this event reports more than [`LONG_SESSION_UPTIME_SECS`] of
/// uptime.
///
/// Both events that carry `uptime_secs` count. `engine_stopped` is the one that
/// sees a short run: the heartbeat interval is six hours and the boot heartbeat
/// fires at two minutes, so a session between 200 seconds and six hours ends
/// without a heartbeat ever reporting past the threshold.
///
/// Strictly greater, so an event that reports exactly the threshold flags
/// nobody. An event without `uptime_secs` reports nothing rather than assuming
/// a zero.
fn reports_long_session(
    event_type: &str,
    properties: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    matches!(event_type, "heartbeat" | "engine_stopped")
        && properties
            .get("uptime_secs")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|uptime| uptime > LONG_SESSION_UPTIME_SECS)
}

fn build_posthog_event(event: ProductEvent) -> PostHogEvent {
    build_posthog_event_with_flag(event, &LONG_SESSION_FLAG_WRITTEN)
}

fn build_posthog_event_with_flag(
    mut event: ProductEvent,
    flag_written: &AtomicBool,
) -> PostHogEvent {
    sanitize_event_properties(&mut event.event_properties);
    if let Some(props) = event.user_properties.as_mut() {
        sanitize_event_properties(props);
    }

    let mut properties = serde_json::Map::new();
    properties.insert("distinct_id".into(), serde_json::json!(event.device_id));
    properties.insert("$process_person_profile".into(), serde_json::json!(false));
    properties.insert(
        "user_mode".into(),
        serde_json::json!(posthog_user_mode(&event.event_type)),
    );
    properties.insert("platform".into(), serde_json::json!(event.platform));
    properties.insert("os_name".into(), serde_json::json!(event.os_name));
    properties.insert("app_version".into(), serde_json::json!(event.app_version));
    if let Some(language) = event.language {
        properties.insert("language".into(), serde_json::json!(language));
    }
    if let Some(serde_json::Value::Object(user_props)) = event.user_properties {
        for (key, value) in user_props {
            if should_skip_posthog_user_property(&key, &properties) {
                continue;
            }
            properties.insert(key, value);
        }
    }
    if let serde_json::Value::Object(event_props) = event.event_properties {
        for (key, value) in event_props {
            if should_skip_posthog_event_property(&key, &properties) {
                continue;
            }
            properties.insert(key, value);
        }
    }

    // The first event past the threshold flags its person. `$set_once` never
    // overwrites, so the flag means "has ever had a long session" rather than
    // "had one recently".
    //
    // Only the first such event in this process asks for it. Person processing
    // makes the event bill as identified, and every later write is a no-op that
    // PostHog charges for, so the flag is claimed once per run.
    //
    // Person properties need person processing, which every other event turns
    // off, so the claiming event turns it back on for itself alone. Without
    // that PostHog drops the `$set_once` and no profile is written.
    if reports_long_session(&event.event_type, &properties)
        && flag_written
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        properties.insert(
            "$set_once".into(),
            serde_json::json!({ LONG_SESSION_PERSON_PROPERTY: true }),
        );
        properties.insert("$process_person_profile".into(), serde_json::json!(true));
    }

    // An identify writes a person property too, so it needs the same person
    // processing. It is not gated on a once-per-process flag: a corrected
    // address has to be able to land.
    if set_person_email(&event.event_type, &mut properties) {
        properties.insert("$process_person_profile".into(), serde_json::json!(true));
    }

    PostHogEvent {
        event: event.event_type,
        properties: serde_json::Value::Object(properties),
        timestamp: posthog_timestamp_millis(event.time),
        uuid: event.insert_id,
    }
}

impl PostHogClient {
    /// Create a new PostHog client with the given project API key and host.
    pub fn new(api_key: String, host: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Failed to build PostHog HTTP client with custom config, using defaults");
                reqwest::Client::default()
            });

        Self {
            api_key,
            host: if host.trim().is_empty() {
                POSTHOG_DEFAULT_HOST.to_string()
            } else {
                host
            },
            client,
        }
    }

    /// Send a single anonymous event to PostHog.
    pub async fn send_event(&self, event: ProductEvent) -> anyhow::Result<()> {
        self.send_batch(vec![event]).await
    }

    /// Send a batch of anonymous events to PostHog.
    /// If the API key is empty, this silently skips sending (for dev/testing).
    /// Returns `Ok(())` even when all retries are exhausted — telemetry is fire-and-forget
    /// and must never block or fail the caller.
    pub async fn send_batch(&self, events: Vec<ProductEvent>) -> anyhow::Result<()> {
        if self.api_key.is_empty() || events.is_empty() {
            return Ok(());
        }

        let payload = PostHogPayload {
            api_key: self.api_key.clone(),
            historical_migration: false,
            batch: events.into_iter().map(build_posthog_event).collect(),
        };

        let url = posthog_batch_url(&self.host);
        let mut delay = std::time::Duration::from_secs(1);

        for attempt in 1..=MAX_RETRIES {
            match self.client.post(&url).json(&payload).send().await {
                Ok(response) if response.status().is_success() => {
                    return Ok(());
                }
                Ok(_) | Err(_) => {}
            }

            if attempt < MAX_RETRIES {
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
        }

        tracing::debug!("PostHog: all retry attempts exhausted, dropping events");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> ProductEvent {
        ProductEvent {
            device_id: "device-1".to_string(),
            user_id: Some("user-1".to_string()),
            event_type: "test_event".to_string(),
            event_properties: serde_json::json!({"key": "value"}),
            user_properties: Some(serde_json::json!({"plan": "free"})),
            platform: "test".to_string(),
            os_name: "linux".to_string(),
            app_version: "0.1.0".to_string(),
            time: 1700000000000,
            insert_id: Some("ins-1".to_string()),
            country: Some("US".to_string()),
            language: Some("en".to_string()),
            ip: Some("$remote".to_string()),
        }
    }

    // =========================================================================
    // Long-session person flag
    // =========================================================================

    /// Builds the event against a flag no other test shares, so each test
    /// sees a process that has not written the flag yet.
    fn unflagged(event: ProductEvent) -> PostHogEvent {
        build_posthog_event_with_flag(event, &AtomicBool::new(false))
    }

    fn heartbeat_with_uptime(uptime_secs: serde_json::Value) -> PostHogEvent {
        let mut event = sample_event();
        event.event_type = "heartbeat".to_string();
        event.event_properties = serde_json::json!({ "uptime_secs": uptime_secs });
        unflagged(event)
    }

    #[test]
    fn a_heartbeat_past_the_threshold_flags_its_person_once() {
        let event = heartbeat_with_uptime(serde_json::json!(201));

        assert_eq!(
            event.properties["$set_once"],
            serde_json::json!({ "uptime_is_greater_than_200_secs": true })
        );
        // Person properties are dropped without person processing, so the
        // flagging event has to ask for it.
        assert_eq!(event.properties["$process_person_profile"], true);
    }

    #[test]
    fn the_threshold_itself_flags_nobody() {
        let event = heartbeat_with_uptime(serde_json::json!(200));

        assert!(event.properties.get("$set_once").is_none());
        assert_eq!(event.properties["$process_person_profile"], false);
    }

    #[test]
    fn a_heartbeat_without_an_uptime_flags_nobody() {
        let mut event = sample_event();
        event.event_type = "heartbeat".to_string();
        event.event_properties = serde_json::json!({});
        let event = unflagged(event);

        assert!(event.properties.get("$set_once").is_none());
        assert_eq!(event.properties["$process_person_profile"], false);
    }

    #[test]
    fn a_stop_past_the_threshold_flags_too() {
        // The run that ends between 200 seconds and the six-hour heartbeat
        // interval is only ever seen by `engine_stopped`.
        let mut event = sample_event();
        event.event_type = "engine_stopped".to_string();
        event.event_properties = serde_json::json!({ "uptime_secs": 900 });
        let event = unflagged(event);

        assert_eq!(
            event.properties["$set_once"],
            serde_json::json!({ "uptime_is_greater_than_200_secs": true })
        );
        assert_eq!(event.properties["$process_person_profile"], true);
    }

    #[test]
    fn a_short_stop_flags_nobody() {
        let mut event = sample_event();
        event.event_type = "engine_stopped".to_string();
        event.event_properties = serde_json::json!({ "uptime_secs": 12 });
        let event = unflagged(event);

        assert!(event.properties.get("$set_once").is_none());
        assert_eq!(event.properties["$process_person_profile"], false);
    }

    #[test]
    fn another_event_with_a_long_uptime_flags_nobody() {
        // Only the two lifecycle events report uptime for this purpose; a
        // future event carrying the same property must not flag by accident.
        let mut event = sample_event();
        event.event_type = "function_invoked".to_string();
        event.event_properties = serde_json::json!({ "uptime_secs": 9_000 });
        let event = unflagged(event);

        assert!(event.properties.get("$set_once").is_none());
        assert_eq!(event.properties["$process_person_profile"], false);
    }

    #[test]
    fn every_other_event_keeps_person_processing_off() {
        let event = unflagged(sample_event());

        assert!(event.properties.get("$set_once").is_none());
        assert_eq!(event.properties["$process_person_profile"], false);
    }

    // =========================================================================
    #[test]
    fn only_the_first_passing_event_of_a_process_writes_the_flag() {
        // A six-hour heartbeat interval still means several passing heartbeats
        // a day from one long-running engine. The property cannot change, so
        // repeats would buy nothing and cost an identified event each.
        let flag = AtomicBool::new(false);
        let heartbeat = || {
            let mut event = sample_event();
            event.event_type = "heartbeat".to_string();
            event.event_properties = serde_json::json!({ "uptime_secs": 9_000 });
            build_posthog_event_with_flag(event, &flag)
        };

        let first = heartbeat();
        assert_eq!(
            first.properties["$set_once"],
            serde_json::json!({ "uptime_is_greater_than_200_secs": true })
        );
        assert_eq!(first.properties["$process_person_profile"], true);

        for _ in 0..3 {
            let later = heartbeat();
            assert!(later.properties.get("$set_once").is_none());
            assert_eq!(later.properties["$process_person_profile"], false);
        }
    }

    // =========================================================================
    // Address redaction in errors
    // =========================================================================

    #[test]
    fn an_address_in_an_error_is_redacted() {
        assert_eq!(
            sanitize_error("could not add someone@example.com to the list"),
            "could not add <redacted> to the list"
        );
        // Inside a path, a URL, and next to punctuation.
        assert_eq!(
            sanitize_error("open /tmp/someone@example.co.uk/x failed"),
            "open /tmp/<redacted>/x failed"
        );
        assert_eq!(
            sanitize_error("POST mailto:first.last+tag@sub.example.com: 400"),
            "POST mailto:<redacted>: 400"
        );
        assert_eq!(
            sanitize_error("two: a@b.com and c@d.org"),
            "two: <redacted> and <redacted>"
        );
    }

    #[test]
    fn text_that_is_not_address_shaped_survives() {
        for error in [
            "no at sign here",
            "user@host has no dotted domain",
            "@example.com is missing a local part",
            "someone@example. ends on a dot",
            "cost was 5@2.5x",
        ] {
            assert_eq!(sanitize_error(error), error, "changed {error}");
        }
    }

    #[test]
    fn the_home_directory_scrub_still_runs() {
        assert_eq!(
            sanitize_error("/Users/dave/oops and someone@example.com"),
            "/Users/<redacted>/oops and <redacted>"
        );
    }

    #[test]
    fn redaction_happens_before_the_length_cap() {
        // An address near the end of a long error must be redacted rather than
        // cut in half and left readable.
        let error = format!("{} someone@example.com", "x".repeat(300));
        let sanitized = sanitize_error(&error);
        assert!(!sanitized.contains("someone@example.com"));
        assert!(!sanitized.contains("someone@"));
    }

    #[test]
    fn the_identify_address_is_never_scrubbed() {
        // The scrubber rewrites properties named `error` only. The identify
        // carries its address under `email`, which is the whole point of this
        // event, so it has to survive the same pipeline that redacts errors.
        let mut event = sample_event();
        event.event_type = IDENTIFY_EVENT.to_string();
        event.event_properties = serde_json::json!({
            "email": "someone@example.com",
            "source": "console_prompt",
            "error": "the list refused someone@example.com",
        });
        let event = unflagged(event);

        assert_eq!(
            event.properties["$set"],
            serde_json::json!({ "email": "someone@example.com" })
        );
        // The same address inside an `error` on the same event is still gone.
        assert_eq!(event.properties["error"], "the list refused <redacted>");
    }

    // =========================================================================
    // Identify
    // =========================================================================

    #[test]
    fn an_identify_writes_the_address_to_the_person() {
        let mut event = sample_event();
        event.event_type = IDENTIFY_EVENT.to_string();
        event.event_properties =
            serde_json::json!({ "email": "someone@example.com", "source": "console_prompt" });
        let event = unflagged(event);

        assert_eq!(
            event.properties["$set"],
            serde_json::json!({ "email": "someone@example.com" })
        );
        assert_eq!(event.properties["$process_person_profile"], true);
        // The address belongs on the person, so it is not left on the event as
        // well. `source` is not an address and stays.
        assert!(event.properties.get("email").is_none());
        assert_eq!(event.properties["source"], "console_prompt");
    }

    #[test]
    fn an_identify_reports_every_time_so_a_corrected_address_lands() {
        // Unlike the long-session flag, this is not claimed once per process:
        // `$set` overwrites, and a typo has to be fixable.
        let flag = AtomicBool::new(false);
        let identify = || {
            let mut event = sample_event();
            event.event_type = IDENTIFY_EVENT.to_string();
            event.event_properties = serde_json::json!({ "email": "second@example.com" });
            build_posthog_event_with_flag(event, &flag)
        };

        for _ in 0..2 {
            let event = identify();
            assert_eq!(
                event.properties["$set"],
                serde_json::json!({ "email": "second@example.com" })
            );
            assert_eq!(event.properties["$process_person_profile"], true);
        }
    }

    #[test]
    fn an_identify_without_an_address_keeps_person_processing_off() {
        let mut event = sample_event();
        event.event_type = IDENTIFY_EVENT.to_string();
        event.event_properties = serde_json::json!({ "source": "console_prompt" });
        let event = unflagged(event);

        assert!(event.properties.get("$set").is_none());
        assert_eq!(event.properties["$process_person_profile"], false);
    }

    #[test]
    fn another_event_carrying_an_email_never_writes_a_person_property() {
        let mut event = sample_event();
        event.event_properties = serde_json::json!({ "email": "someone@example.com" });
        let event = unflagged(event);

        assert!(event.properties.get("$set").is_none());
        assert_eq!(event.properties["$process_person_profile"], false);
    }

    // =========================================================================
    // ProductEvent serialization
    // =========================================================================

    #[test]
    fn test_event_serialization_required_fields() {
        let event = sample_event();
        let json = serde_json::to_value(&event).unwrap();

        assert_eq!(json["device_id"], "device-1");
        assert_eq!(json["user_id"], "user-1");
        assert_eq!(json["event_type"], "test_event");
        assert_eq!(json["event_properties"]["key"], "value");
        assert_eq!(json["platform"], "test");
        assert_eq!(json["os_name"], "linux");
        assert_eq!(json["app_version"], "0.1.0");
        assert_eq!(json["time"], 1700000000000i64);
        assert_eq!(json["insert_id"], "ins-1");
        assert_eq!(json["country"], "US");
        assert_eq!(json["language"], "en");
        assert_eq!(json["ip"], "$remote");
    }

    #[test]
    fn test_event_serialization_skip_none_fields() {
        let event = ProductEvent {
            device_id: "d1".to_string(),
            user_id: None,
            event_type: "evt".to_string(),
            event_properties: serde_json::json!({}),
            user_properties: None,
            platform: "test".to_string(),
            os_name: "macos".to_string(),
            app_version: "1.0.0".to_string(),
            time: 0,
            insert_id: None,
            country: None,
            language: None,
            ip: None,
        };

        let json = serde_json::to_value(&event).unwrap();

        // Fields with skip_serializing_if = "Option::is_none" should be absent
        assert!(
            json.get("user_id").is_none(),
            "user_id=None should be skipped"
        );
        assert!(
            json.get("user_properties").is_none(),
            "user_properties=None should be skipped"
        );
        assert!(
            json.get("country").is_none(),
            "country=None should be skipped"
        );
        assert!(
            json.get("language").is_none(),
            "language=None should be skipped"
        );
        assert!(json.get("ip").is_none(), "ip=None should be skipped");

        // Required fields should still be present
        assert!(json.get("device_id").is_some());
        assert!(json.get("event_type").is_some());
        assert!(json.get("event_properties").is_some());
        assert!(json.get("platform").is_some());
        assert!(json.get("os_name").is_some());
        assert!(json.get("app_version").is_some());
        assert!(json.get("time").is_some());
    }

    #[test]
    fn test_event_clone() {
        let event = sample_event();
        let cloned = event.clone();
        assert_eq!(event.device_id, cloned.device_id);
        assert_eq!(event.event_type, cloned.event_type);
        assert_eq!(event.time, cloned.time);
        assert_eq!(event.insert_id, cloned.insert_id);
    }

    #[test]
    fn test_event_debug_format() {
        let event = sample_event();
        let debug = format!("{:?}", event);
        assert!(debug.contains("ProductEvent"));
        assert!(debug.contains("test_event"));
        assert!(debug.contains("device-1"));
    }

    #[test]
    fn test_event_roundtrip_json() {
        let event = sample_event();
        let json_str = serde_json::to_string(&event).unwrap();
        // Verify it's valid JSON by parsing it back
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.is_object());
        assert_eq!(parsed["event_type"], "test_event");
    }

    #[test]
    fn test_posthog_user_mode_classifies_runtime_as_using() {
        assert_eq!(posthog_user_mode("heartbeat"), "using");
        assert_eq!(posthog_user_mode("engine_stopped"), "using");
    }

    #[test]
    fn test_posthog_user_mode_classifies_cli_and_scaffolding_as_building() {
        assert_eq!(posthog_user_mode("install_started"), "building");
        assert_eq!(posthog_user_mode("cli_update_started"), "building");
        assert_eq!(posthog_user_mode("project_created"), "building");
        assert_eq!(posthog_user_mode("template_success"), "building");
    }

    #[test]
    fn test_posthog_payload_is_anonymous_batch_shape() {
        let payload = PostHogPayload {
            api_key: "phc_test".to_string(),
            historical_migration: false,
            batch: vec![build_posthog_event(sample_event())],
        };

        let json = serde_json::to_value(&payload).unwrap();
        let event = &json["batch"][0];

        assert_eq!(json["api_key"], "phc_test");
        assert_eq!(json["historical_migration"], false);
        assert_eq!(event["event"], "test_event");
        assert_eq!(event["uuid"], "ins-1");
        assert_eq!(event["properties"]["distinct_id"], "device-1");
        assert_eq!(event["properties"]["$process_person_profile"], false);
        assert_eq!(event["properties"]["user_mode"], "building");
        assert_eq!(event["properties"]["key"], "value");
        assert_eq!(event["properties"]["plan"], "free");
    }

    #[test]
    fn test_posthog_payload_dedupes_version_and_function_fields() {
        let event = ProductEvent {
            device_id: "device-1".to_string(),
            user_id: None,
            event_type: "heartbeat".to_string(),
            event_properties: serde_json::json!({
                "version": "0.13.0-next.1",
                "function_names": ["orders::charge", "agent::memory"],
                "functions": ["orders::charge", "agent::memory"],
                "project_name": "agentmemory",
            }),
            user_properties: Some(serde_json::json!({
                "iii_version": "0.13.0-next.1",
                "project_name": "agentmemory",
            })),
            platform: "iii-engine".to_string(),
            os_name: "linux".to_string(),
            app_version: "0.13.0-next.1".to_string(),
            time: 1700000000000,
            insert_id: Some("ins-2".to_string()),
            country: None,
            language: None,
            ip: None,
        };

        let props = build_posthog_event(event).properties;
        let props = props.as_object().expect("properties object");

        assert_eq!(props.get("app_version").unwrap(), "0.13.0-next.1");
        assert!(!props.contains_key("iii_version"));
        assert!(!props.contains_key("version"));
        assert!(!props.contains_key("function_names"));
        assert_eq!(
            props.get("functions").unwrap(),
            &serde_json::json!(["orders::charge", "agent::memory"])
        );
        assert_eq!(props.get("project_name").unwrap(), "agentmemory");
    }

    #[tokio::test]
    async fn test_posthog_send_batch_empty_api_key_is_noop() {
        let client = PostHogClient::new(String::new(), POSTHOG_DEFAULT_HOST.to_string());
        let result = client.send_batch(vec![sample_event()]).await;
        assert!(
            result.is_ok(),
            "empty PostHog API key should silently succeed"
        );
    }

    #[tokio::test]
    async fn test_posthog_send_batch_empty_events_is_noop() {
        let client = PostHogClient::new("phc_test".to_string(), POSTHOG_DEFAULT_HOST.to_string());
        let result = client.send_batch(vec![]).await;
        assert!(
            result.is_ok(),
            "empty PostHog events vec should silently succeed"
        );
    }

    // =========================================================================
    // Constants
    // =========================================================================

    #[test]
    fn test_max_retries_is_three() {
        assert_eq!(MAX_RETRIES, 3);
    }

    #[test]
    fn sanitize_error_redacts_unix_users_path() {
        let s = sanitize_error("failed to open /Users/alice/secret.txt: not found");
        assert!(!s.contains("alice"), "username should be redacted: {s}");
        assert!(s.contains("/Users/<redacted>/"));
    }

    #[test]
    fn sanitize_error_redacts_unix_home_path() {
        let s = sanitize_error("permission denied for /home/bob/.ssh/id_rsa");
        assert!(!s.contains("bob"), "username should be redacted: {s}");
        assert!(s.contains("/home/<redacted>/"));
    }

    #[test]
    fn sanitize_error_redacts_windows_users_path() {
        let s = sanitize_error("open C:\\Users\\carol\\secret.txt failed");
        assert!(!s.contains("carol"), "username should be redacted: {s}");
        assert!(s.contains("\\Users\\<redacted>\\"));
    }

    #[test]
    fn sanitize_error_truncates_long_strings() {
        let long = "x".repeat(1024);
        let s = sanitize_error(&long);
        let len = s.chars().count();
        assert!(len <= 257, "truncated length should be <= 257, got {len}");
        assert!(
            s.ends_with("…"),
            "truncated output should end with ellipsis"
        );
    }

    #[test]
    fn sanitize_error_passes_through_safe_strings() {
        let s = sanitize_error("HTTP 500: Internal Server Error");
        assert_eq!(s, "HTTP 500: Internal Server Error");
    }

    #[test]
    fn sanitize_event_properties_walks_nested_error_fields() {
        let mut value = serde_json::json!({
            "stage": "create_dir",
            "error": "/Users/dave/oops",
            "nested": {"error": "/home/eve/oops"},
            "list": [{"error": "/Users/frank/x"}],
        });
        sanitize_event_properties(&mut value);
        assert!(!value.to_string().contains("dave"));
        assert!(!value.to_string().contains("eve"));
        assert!(!value.to_string().contains("frank"));
    }
}
