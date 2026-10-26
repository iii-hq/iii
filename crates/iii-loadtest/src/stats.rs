// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Latency histograms and what a step reports about them.

use hdrhistogram::Histogram;
use serde::{Deserialize, Serialize};

/// Microseconds, from 1 µs to 120 s at 3 significant digits.
pub fn histogram() -> Histogram<u64> {
    Histogram::new_with_bounds(1, 120_000_000, 3).expect("valid histogram bounds")
}

pub fn record(hist: &mut Histogram<u64>, elapsed: std::time::Duration) {
    let us = (elapsed.as_micros() as u64).clamp(1, 120_000_000);
    hist.record(us).ok();
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Latency {
    pub mean_us: f64,
    pub p50_us: u64,
    pub p90_us: u64,
    pub p99_us: u64,
    pub p999_us: u64,
    pub max_us: u64,
}

impl Latency {
    pub fn of(hist: &Histogram<u64>) -> Option<Self> {
        (!hist.is_empty()).then(|| Latency {
            mean_us: hist.mean(),
            p50_us: hist.value_at_quantile(0.50),
            p90_us: hist.value_at_quantile(0.90),
            p99_us: hist.value_at_quantile(0.99),
            p999_us: hist.value_at_quantile(0.999),
            max_us: hist.max(),
        })
    }
}

/// What a load ran into: calls that succeeded (timed in `hist`), calls that
/// failed, and the first failure, to say why.
#[derive(Debug, Clone)]
pub struct Load {
    pub ops: u64,
    pub errors: u64,
    pub hist: Histogram<u64>,
    pub first_error: Option<String>,
}

impl Default for Load {
    fn default() -> Self {
        Self {
            ops: 0,
            errors: 0,
            hist: histogram(),
            first_error: None,
        }
    }
}

impl Load {
    pub fn ok(&mut self, elapsed: std::time::Duration) {
        self.ops += 1;
        record(&mut self.hist, elapsed);
    }

    pub fn err(&mut self, error: impl std::fmt::Display) {
        self.errors += 1;
        if self.first_error.is_none() {
            self.first_error = Some(error.to_string());
        }
    }

    pub fn merge(&mut self, other: Load) {
        self.ops += other.ops;
        self.errors += other.errors;
        self.hist.add(&other.hist).ok();
        if self.first_error.is_none() {
            self.first_error = other.first_error;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn loads_merge_counts_histograms_and_the_first_error() {
        let mut a = Load::default();
        a.ok(Duration::from_micros(100));
        a.err("first");
        let mut b = Load::default();
        b.ok(Duration::from_micros(300));
        b.err("second");
        a.merge(b);
        assert_eq!((a.ops, a.errors), (2, 2));
        assert_eq!(a.first_error.as_deref(), Some("first"));
        let lat = Latency::of(&a.hist).unwrap();
        assert_eq!(lat.p50_us, 100);
        assert!(lat.max_us >= 300 && lat.max_us < 301);
        assert!(Latency::of(&histogram()).is_none());
    }
}
