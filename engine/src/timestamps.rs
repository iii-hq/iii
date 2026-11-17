// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Compatibility for the engine's existing UTC timestamp representations.

use jiff::Timestamp;
use serde::{Serialize, Serializer};

// Chrono emitted fractional seconds in groups of three digits, not trimmed
// decimal digits. Preserve that representation in persisted state and HTTP metadata.
fn precision(timestamp: Timestamp) -> usize {
    let nanos = timestamp.subsec_nanosecond();
    if nanos == 0 {
        0
    } else if nanos % 1_000_000 == 0 {
        3
    } else if nanos % 1_000 == 0 {
        6
    } else {
        9
    }
}

/// Serialize an RFC 3339 UTC timestamp with the existing fractional precision.
pub fn serialize<S: Serializer>(timestamp: &Timestamp, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(&format_args!(
        "{timestamp:.precision$}",
        precision = precision(*timestamp)
    ))
}

/// Serialization of optional timestamps, including JSON null.
pub mod optional {
    use super::*;

    /// Serialize an optional UTC timestamp without changing its wire format.
    pub fn serialize<S: Serializer>(
        timestamp: &Option<Timestamp>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        struct UtcTimestamp(Timestamp);
        impl Serialize for UtcTimestamp {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                super::serialize(&self.0, serializer)
            }
        }
        timestamp.map(UtcTimestamp).serialize(serializer)
    }
}

/// PostHog uses an explicit UTC offset instead of the persisted form's `Z`.
pub(crate) fn format_with_offset(timestamp: Timestamp) -> String {
    format!(
        "{:.precision$}",
        timestamp.display_with_offset(jiff::tz::Offset::UTC),
        precision = precision(timestamp)
    )
}

/// Unix milliseconds round down before the epoch, as the previous clock did.
pub(crate) fn unix_millis(timestamp: Timestamp) -> i64 {
    timestamp.as_nanosecond().div_euclid(1_000_000) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Persisted {
        #[serde(serialize_with = "super::serialize")]
        timestamp: Timestamp,
    }

    #[test]
    fn timestamp_serde_preserves_utc_fractional_precision_and_offsets() {
        for (input, expected) in [
            ("2026-01-02T03:04:05Z", "2026-01-02T03:04:05Z"),
            ("2026-01-02T03:04:05.1Z", "2026-01-02T03:04:05.100Z"),
            ("2026-01-02T03:04:05.1234Z", "2026-01-02T03:04:05.123400Z"),
            (
                "2026-01-02T03:04:05.123456789Z",
                "2026-01-02T03:04:05.123456789Z",
            ),
            ("2026-01-02T04:04:05.12+01:00", "2026-01-02T03:04:05.120Z"),
            ("1969-12-31T23:59:59.9Z", "1969-12-31T23:59:59.900Z"),
        ] {
            let value: Persisted =
                serde_json::from_value(serde_json::json!({"timestamp": input})).unwrap();
            assert_eq!(serde_json::to_value(value).unwrap()["timestamp"], expected);
        }
    }

    #[test]
    fn timestamp_unix_millis_preserves_pre_epoch_floor() {
        for (input, expected) in [
            ("1970-01-01T00:00:00.001999Z", 1),
            ("1969-12-31T23:59:59.999999Z", -1),
        ] {
            assert_eq!(unix_millis(input.parse().unwrap()), expected);
        }
    }
}
