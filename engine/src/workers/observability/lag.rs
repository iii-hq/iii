// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Reporting that a consumer of the log broadcast fell behind.
//!
//! A consumer of the engine's log channel must not log every lag. The report
//! is a log line itself: it lands in the same full channel, evicts the message
//! the consumer was about to read, and the consumer lags again, by one, and
//! reports again. One burst of logs (a thousand workers disconnecting at
//! `info`) then keeps a core busy and the log growing for as long as the engine
//! runs. [`LagReport`] reports at most once per [`LAG_REPORT_INTERVAL`], so the
//! consumer catches up between two reports.
//!
//! What a consumer skips between two reports is held back, not lost: it is
//! reported with the next lag past the interval, or when the interval ends
//! (the consumer waits on [`LagReport::deadline`] with [`sleep_until`]), or
//! when the consumer stops ([`LagReport::rest`]). Every skip is reported once.

use std::time::Duration;

use tokio::time::Instant;

/// The shortest time between two lag reports of one consumer.
pub(crate) const LAG_REPORT_INTERVAL: Duration = Duration::from_secs(10);

/// The messages a consumer skipped since it last said so.
#[derive(Debug, Default)]
pub(crate) struct LagReport {
    skipped: u64,
    last: Option<Instant>,
}

impl LagReport {
    /// Records `skipped` messages. Returns how many to report now (those and
    /// any held back since the last report), or `None` while the last report
    /// is more recent than [`LAG_REPORT_INTERVAL`].
    pub(crate) fn lagged(&mut self, skipped: u64) -> Option<u64> {
        self.lagged_at(skipped, Instant::now())
    }

    fn lagged_at(&mut self, skipped: u64, now: Instant) -> Option<u64> {
        self.skipped = self.skipped.saturating_add(skipped);
        if self
            .last
            .is_some_and(|last| now.duration_since(last) < LAG_REPORT_INTERVAL)
        {
            return None;
        }
        self.last = Some(now);
        Some(std::mem::take(&mut self.skipped))
    }

    /// When the skips held back may be reported, if any are: the consumer
    /// wakes then ([`sleep_until`]) and calls [`Self::due`], so the last skips
    /// of a burst are reported even when no lag follows them.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        match self.last {
            Some(last) if self.skipped > 0 => Some(last + LAG_REPORT_INTERVAL),
            _ => None,
        }
    }

    /// The skips held back, once [`LAG_REPORT_INTERVAL`] has passed since the
    /// last report; `None` before that, or when there are none.
    pub(crate) fn due(&mut self) -> Option<u64> {
        self.due_at(Instant::now())
    }

    fn due_at(&mut self, now: Instant) -> Option<u64> {
        if self.deadline().is_none_or(|deadline| now < deadline) {
            return None;
        }
        self.last = Some(now);
        Some(std::mem::take(&mut self.skipped))
    }

    /// The skips held back, whenever it is: the consumer is stopping.
    pub(crate) fn rest(&mut self) -> Option<u64> {
        (self.skipped > 0).then(|| std::mem::take(&mut self.skipped))
    }
}

/// Resolves at `deadline`, and never without one: a consumer's `select!`
/// branch for [`LagReport::deadline`].
pub(crate) async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast::{self, error::TryRecvError};

    #[test]
    fn lags_are_reported_once_per_interval_with_what_was_held_back() {
        let t0 = Instant::now();
        let mut report = LagReport::default();
        assert_eq!(report.lagged_at(3, t0), Some(3));
        assert_eq!(report.lagged_at(1, t0 + Duration::from_secs(1)), None);
        assert_eq!(report.lagged_at(2, t0 + Duration::from_secs(9)), None);
        assert_eq!(
            report.lagged_at(1, t0 + LAG_REPORT_INTERVAL + Duration::from_secs(1)),
            Some(4)
        );
    }

    /// The skips held back after a report are due when its interval ends,
    /// with no further lag, and are reported once.
    #[test]
    fn held_back_skips_are_due_when_the_interval_ends() {
        let t0 = Instant::now();
        let mut report = LagReport::default();
        assert_eq!(report.deadline(), None);
        assert_eq!(report.lagged_at(3, t0), Some(3));
        assert_eq!(report.deadline(), None, "nothing held back");
        assert_eq!(report.lagged_at(2, t0 + Duration::from_secs(1)), None);
        assert_eq!(report.deadline(), Some(t0 + LAG_REPORT_INTERVAL));
        assert_eq!(report.due_at(t0 + Duration::from_secs(9)), None);
        assert_eq!(report.due_at(t0 + LAG_REPORT_INTERVAL), Some(2));
        assert_eq!(report.deadline(), None);
        assert_eq!(report.due_at(t0 + 3 * LAG_REPORT_INTERVAL), None);
        // That report starts a new interval.
        assert_eq!(
            report.lagged_at(1, t0 + LAG_REPORT_INTERVAL + Duration::from_secs(1)),
            None
        );
        assert_eq!(report.rest(), Some(1));
        assert_eq!(report.rest(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn sleep_until_waits_for_the_deadline_and_forever_without_one() {
        let start = Instant::now();
        sleep_until(Some(start + LAG_REPORT_INTERVAL)).await;
        assert_eq!(start.elapsed(), LAG_REPORT_INTERVAL);
        let never = tokio::time::timeout(Duration::from_secs(3600), sleep_until(None)).await;
        assert!(never.is_err());
    }

    /// The consumer of a full channel, reporting each lag into that same
    /// channel the way a `tracing::warn!` lands in the log broadcast. Returns
    /// how many messages it received and how many reports it sent.
    fn consume(mut report: impl FnMut(u64) -> bool) -> (u32, u32) {
        let (tx, mut rx) = broadcast::channel::<u32>(4);
        // A burst: the consumer is now behind.
        for i in 0..16 {
            tx.send(i).unwrap();
        }
        let (mut received, mut reports) = (0, 0);
        for _ in 0..10_000 {
            match rx.try_recv() {
                Ok(_) => received += 1,
                Err(TryRecvError::Lagged(n)) => {
                    if report(n) {
                        reports += 1;
                        tx.send(u32::MAX).unwrap();
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Closed) => unreachable!(),
            }
        }
        (received, reports)
    }

    #[test]
    fn reporting_every_lag_into_the_lagging_channel_never_catches_up() {
        // What the subscribers did: each report evicts the next message.
        let (received, reports) = consume(|_| true);
        assert_eq!(received, 0);
        assert_eq!(reports, 10_000);
    }

    #[test]
    fn a_rate_limited_report_lets_the_consumer_catch_up() {
        let mut report = LagReport::default();
        let (received, reports) = consume(|n| report.lagged(n).is_some());
        assert_eq!(reports, 1);
        // The channel's whole backlog, the report included.
        assert_eq!(received, 4);
    }
}
