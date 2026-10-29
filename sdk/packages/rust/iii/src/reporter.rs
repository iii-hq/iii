//! The report buffer: what the host's delivery matched in local calls, on
//! its way to the engine as `invocationsobserved` frames.
//!
//! - **Bounded.** 10 000 items or 8 MiB. When full it drops the newest item
//!   and counts it (`BUFFER_FULL`); an item larger than a frame is dropped
//!   and counted as `TOO_LARGE`. Pushing never blocks and never waits on the
//!   socket.
//! - **Batched.** A flush happens at 256 items or after a 5 ms linger,
//!   whichever comes first, and every frame stays under the 16 MiB limit.
//! - **Kept while disconnected.** Items wait in the same bounded buffer and
//!   are sent once the engine has confirmed their function again on the new
//!   connection. Frames already flushed but not written when the connection
//!   dropped come back as items; one whose write failed is not resent (it may
//!   have arrived: reports are at-most-once) and counts as `INACTIVE`.
//!
//! A routed call's items skip the buffer: they go out at once, just ahead of
//! the call's answer ([`Reporter::frames_of`]).

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::value::RawValue;
use tokio::sync::Notify;

use crate::protocol::{Caller, InvocationStatus, observe_codes};

/// Items kept at most before dropping the newest.
pub(crate) const MAX_ITEMS: usize = 10_000;
/// Estimated bytes kept at most before dropping the newest.
pub(crate) const MAX_BYTES: usize = 8 * 1024 * 1024;
/// Items per `invocationsobserved` frame, and the count that triggers a flush.
pub(crate) const BATCH_ITEMS: usize = 256;
/// How long the first item of a batch may wait for more.
pub(crate) const LINGER: Duration = Duration::from_millis(5);
/// How often `localinvocationstats` is sent (only when non-zero).
pub(crate) const STATS_INTERVAL: Duration = Duration::from_secs(10);

/// `{"type":"invocationsobserved","items":[` ... `]}`
const FRAME_PREFIX: &str = r#"{"type":"invocationsobserved","items":["#;
const FRAME_SUFFIX: &str = "]}";
/// Fixed per-item allowance for keys, ids, numbers and punctuation.
const ITEM_OVERHEAD: usize = 256;

/// Which optional parts of an item travel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CaptureMask {
    pub input: bool,
    pub output: bool,
    pub error: bool,
    pub caller: bool,
    pub metadata: bool,
}

impl CaptureMask {
    pub(crate) fn union(self, other: CaptureMask) -> CaptureMask {
        CaptureMask {
            input: self.input || other.input,
            output: self.output || other.output,
            error: self.error || other.error,
            caller: self.caller || other.caller,
            metadata: self.metadata || other.metadata,
        }
    }
}

/// One item waiting in the buffer. Its parts are already serialized, so a
/// flush only concatenates.
#[derive(Debug)]
pub(crate) struct PendingItem {
    pub function_id: String,
    pub namespace: Option<String>,
    pub invocation_id: String,
    pub matched: Vec<String>,
    pub status: InvocationStatus,
    pub input: Option<Box<RawValue>>,
    pub output: Option<Box<RawValue>>,
    pub error: Option<Box<RawValue>>,
    pub caller: Option<Caller>,
    pub metadata: Option<Box<RawValue>>,
    pub started_at_ms: u64,
    pub duration_ms: f64,
    pub observe_depth: u32,
    pub traceparent: Option<String>,
    pub baggage: Option<String>,
    /// What the item ships of the parts above.
    pub mask: CaptureMask,
    /// The handler returned its input: `input` carries the value once.
    pub output_is_input: bool,
    /// Push order, set by [`Reporter::push`] and kept wherever the item goes
    /// (parked, flushed, read back from a frame after the connection
    /// dropped); see [`merge_in`]. 0 for an item read back from a frame that
    /// carries no seqs.
    pub seq: u64,
}

#[cfg(test)]
impl PendingItem {
    /// A successful invocation of `function_id` matched by observer `o`.
    pub(crate) fn test(function_id: &str) -> Self {
        PendingItem {
            function_id: function_id.to_string(),
            namespace: Some("default".to_string()),
            invocation_id: "inv".to_string(),
            matched: vec!["o".to_string()],
            status: InvocationStatus::Success,
            input: None,
            output: None,
            error: None,
            caller: None,
            metadata: None,
            started_at_ms: 1,
            duration_ms: 0.5,
            observe_depth: 0,
            traceparent: None,
            baggage: None,
            mask: CaptureMask::default(),
            output_is_input: false,
            seq: 0,
        }
    }
}

impl PendingItem {
    /// An estimate of the encoded size, used for the byte bound.
    pub(crate) fn estimated_size(&self) -> usize {
        fn raw(value: &Option<Box<RawValue>>) -> usize {
            value.as_ref().map_or(0, |v| v.get().len())
        }
        fn text(value: &Option<String>) -> usize {
            value.as_ref().map_or(0, String::len)
        }
        let caller = self.caller.as_ref().map_or(0, |c| {
            text(&c.worker_id)
                + text(&c.worker_name)
                + text(&c.function_id)
                + text(&c.invocation_id)
                + c.parent.as_ref().map_or(0, |p| {
                    text(&p.worker_id)
                        + text(&p.worker_name)
                        + text(&p.function_id)
                        + text(&p.invocation_id)
                        + 96
                })
                + 96
        });
        ITEM_OVERHEAD
            + self.function_id.len()
            + text(&self.namespace)
            + self.invocation_id.len()
            + self.matched.iter().map(|id| id.len() + 3).sum::<usize>()
            + raw(&self.input)
            + raw(&self.output)
            + raw(&self.error)
            + raw(&self.metadata)
            + caller
            + text(&self.traceparent)
            + text(&self.baggage)
    }

    /// The item as it goes on the wire. To an engine without `observe.v2`
    /// (`v2: false`), an item whose output is its input carries it as
    /// `output` too.
    pub(crate) fn to_json(&self, v2: bool) -> String {
        #[derive(Serialize)]
        struct Wire<'a> {
            function_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            namespace: Option<&'a str>,
            invocation_id: &'a str,
            matched: &'a [String],
            status: &'a InvocationStatus,
            #[serde(skip_serializing_if = "Option::is_none")]
            input: Option<&'a RawValue>,
            #[serde(skip_serializing_if = "Option::is_none")]
            output: Option<&'a RawValue>,
            #[serde(skip_serializing_if = "Option::is_none")]
            error: Option<&'a RawValue>,
            #[serde(skip_serializing_if = "Option::is_none")]
            caller: Option<&'a Caller>,
            #[serde(skip_serializing_if = "Option::is_none")]
            metadata: Option<&'a RawValue>,
            started_at_ms: u64,
            duration_ms: f64,
            observe_depth: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            traceparent: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            baggage: Option<&'a str>,
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            output_is_input: bool,
        }
        let mask = self.mask;
        let output = if self.output_is_input && !v2 {
            self.input.as_deref()
        } else {
            self.output.as_deref()
        };
        let wire = Wire {
            function_id: &self.function_id,
            namespace: self.namespace.as_deref(),
            invocation_id: &self.invocation_id,
            matched: &self.matched,
            status: &self.status,
            input: self.input.as_deref().filter(|_| mask.input),
            output: output.filter(|_| mask.output),
            error: self.error.as_deref().filter(|_| mask.error),
            caller: self.caller.as_ref().filter(|_| mask.caller),
            metadata: self.metadata.as_deref().filter(|_| mask.metadata),
            started_at_ms: self.started_at_ms,
            duration_ms: self.duration_ms,
            observe_depth: self.observe_depth,
            traceparent: self.traceparent.as_deref(),
            baggage: self.baggage.as_deref(),
            output_is_input: self.output_is_input && v2 && mask.output,
        };
        // Every part is either a plain value or already valid JSON.
        serde_json::to_string(&wire).expect("report item serializes")
    }
}

/// An `invocationsobserved` frame read back.
#[derive(serde::Deserialize)]
struct FrameIn {
    items: Vec<crate::protocol::ObservedItem>,
}

impl PendingItem {
    /// An item of a frame read back, with every part it carries.
    fn from_observed(item: crate::protocol::ObservedItem) -> Self {
        let raw = |value: Option<serde_json::Value>| {
            value.and_then(|v| serde_json::value::to_raw_value(&v).ok())
        };
        PendingItem {
            function_id: item.function_id,
            namespace: item.namespace,
            invocation_id: item.invocation_id,
            matched: item.matched,
            status: item.status,
            input: raw(item.input),
            output: raw(item.output),
            error: raw(item.error),
            caller: item.caller,
            metadata: raw(item.metadata),
            started_at_ms: item.started_at_ms,
            duration_ms: item.duration_ms,
            observe_depth: item.observe_depth,
            traceparent: item.traceparent,
            baggage: item.baggage,
            mask: CaptureMask {
                input: true,
                output: true,
                error: true,
                caller: true,
                metadata: true,
            },
            output_is_input: item.output_is_input,
            seq: 0,
        }
    }
}

/// What to do with a buffered item at flush time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// The engine confirmed its function on this connection.
    Send,
    /// Its function is not confirmed (yet) on this connection: park the item
    /// until [`Reporter::unpark`].
    Hold,
    /// Its function is gone: drop and count as `INACTIVE`.
    Drop,
}

#[derive(Debug, Default)]
struct State {
    /// Items not looked at by a flush yet, oldest first.
    items: VecDeque<PendingItem>,
    /// Items whose function waits for its confirmation on this connection,
    /// with when the function's first item was parked.
    parked: HashMap<String, (Instant, Vec<PendingItem>)>,
    /// Items in `items` and `parked`.
    count: usize,
    /// Estimated bytes of `items` and `parked`.
    bytes: usize,
    /// When the oldest item of `items` arrived, for the linger.
    oldest: Option<Instant>,
    drops: HashMap<(String, &'static str), u64>,
    /// When each function's held items were last released (`discard`,
    /// expiry) and unparked, as `fate_seq` values: a flush re-checks them
    /// before parking items it took out of the queue earlier. Only meaningful
    /// within one flush, so each flush clears it.
    fates: HashMap<String, Fates>,
    fate_seq: u64,
    /// A flush has items out of the queue: only then do fates matter.
    taking: bool,
    /// The next [`PendingItem::seq`].
    next_seq: u64,
    /// The worker stopped: nothing will be sent any more.
    closed: bool,
}

/// Merges `incoming` into `items`, both in [`PendingItem::seq`] order.
///
/// Same-function order rests on two invariants, not on when things happen:
/// every list of items (`items`, each parked list) is kept in `seq` order,
/// and an item keeps its `seq` wherever it goes (parked, unparked, flushed
/// into a frame whose lease carries the seqs, read back from that frame
/// when the connection drops). Every move back into line is this merge, so
/// any interleaving of reconnects, confirmations and flushes, across any
/// number of reconnects, still sends a function's items in push order.
/// Ties (items read back without seqs, at 0) keep `items` first.
fn merge_in(items: &mut VecDeque<PendingItem>, incoming: Vec<PendingItem>) {
    let (Some(first), Some(last)) = (incoming.first(), incoming.last()) else {
        return;
    };
    let (first, last) = (first.seq, last.seq);
    // The usual cases cost what they move: all newer than what waits...
    if items.back().is_none_or(|newest| newest.seq <= first) {
        items.extend(incoming);
        return;
    }
    // ...or all older (unparked after a confirmation, read back).
    if items.front().is_some_and(|oldest| last < oldest.seq) {
        items.reserve(incoming.len());
        for item in incoming.into_iter().rev() {
            items.push_front(item);
        }
        return;
    }
    // Interleaved: merge from where the first incoming item goes.
    let at = items.partition_point(|item| item.seq <= first);
    let mut tail = items.split_off(at).into_iter().peekable();
    let mut incoming = incoming.into_iter().peekable();
    loop {
        let take_incoming = match (tail.peek(), incoming.peek()) {
            (Some(item), Some(other)) => other.seq < item.seq,
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (None, None) => break,
        };
        let next = if take_incoming {
            incoming.next()
        } else {
            tail.next()
        };
        items.extend(next);
    }
}

/// Puts `item` in `list` in [`PendingItem::seq`] order (at the end, usually).
fn insert_in_order(list: &mut Vec<PendingItem>, item: PendingItem) {
    let at = list.partition_point(|other| other.seq <= item.seq);
    list.insert(at, item);
}

/// Distinct `(function, reason)` drop counters kept between two stats
/// windows; more are counted under the function `*`.
const MAX_DROP_KEYS: usize = 1024;

#[derive(Debug, Clone, Copy, Default)]
struct Fates {
    /// Gone or never to be confirmed: its items are dropped.
    released: u64,
    /// Confirmed on the current connection: its items go out.
    unparked: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    Unparked,
    Released,
}

impl State {
    fn count(&mut self, function_id: &str, reason: &'static str, n: u64) {
        let key = if self.drops.len() < MAX_DROP_KEYS
            || self.drops.contains_key(&(function_id.to_string(), reason))
        {
            (function_id.to_string(), reason)
        } else {
            ("*".to_string(), reason)
        };
        *self.drops.entry(key).or_insert(0) += n;
    }

    fn record(&mut self, function_id: &str, fate: Fate) {
        if !self.taking {
            return;
        }
        self.fate_seq += 1;
        let seq = self.fate_seq;
        let fates = self.fates.entry(function_id.to_string()).or_default();
        match fate {
            Fate::Unparked => fates.unparked = seq,
            Fate::Released => fates.released = seq,
        }
    }
}

/// Limits, overridable in tests.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub max_items: usize,
    pub max_bytes: usize,
    pub batch_items: usize,
    pub frame_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_items: MAX_ITEMS,
            max_bytes: MAX_BYTES,
            batch_items: BATCH_ITEMS,
            frame_bytes: crate::iii::MAX_JSON_FRAME_BYTES,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Reporter {
    state: Mutex<State>,
    limits: Limits,
    /// Bytes of flushed frames not written to the socket yet (queued behind a
    /// slow or stalled peer, or held across a reconnect). They count against
    /// the same byte bound as buffered items.
    in_flight: Arc<AtomicUsize>,
    /// Wakes the flush loop: the first item of a batch, a full batch, a
    /// connection that came up, a function confirmed again.
    pub(crate) wake: Notify,
}

/// One `invocationsobserved` frame, with its bytes held against the bound.
#[derive(Debug)]
pub(crate) struct ReportFrame {
    pub json: String,
    pub lease: ReportLease,
    /// The function of each item, to count them if the frame is lost.
    pub functions: Vec<String>,
}

/// Holds a flushed frame's bytes against the bound until the frame is
/// written or dropped.
#[derive(Debug)]
pub(crate) struct ReportLease {
    in_flight: Arc<AtomicUsize>,
    bytes: usize,
    /// The [`PendingItem::seq`] of each item of the frame, so a requeue puts
    /// them back in their place.
    pub seqs: Vec<u64>,
}

impl Drop for ReportLease {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

impl Default for Reporter {
    fn default() -> Self {
        Self::with_limits(Limits::default())
    }
}

impl Reporter {
    pub(crate) fn with_limits(limits: Limits) -> Self {
        Self {
            state: Mutex::new(State::default()),
            limits,
            in_flight: Arc::new(AtomicUsize::new(0)),
            wake: Notify::new(),
        }
    }

    /// Counts `bytes` of a flushed frame until the lease is dropped.
    pub(crate) fn lease(&self, bytes: usize) -> ReportLease {
        self.in_flight.fetch_add(bytes, Ordering::AcqRel);
        ReportLease {
            in_flight: self.in_flight.clone(),
            bytes,
            seqs: Vec::new(),
        }
    }

    /// Bytes flushed but not written yet.
    pub(crate) fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// An item above this is `TOO_LARGE`: it fits neither a frame nor the
    /// buffer.
    fn largest_item(&self) -> usize {
        (self.limits.frame_bytes - FRAME_PREFIX.len() - FRAME_SUFFIX.len())
            .min(self.limits.max_bytes)
    }

    /// Whether one more item of `size` bytes overflows the buffer.
    fn full(&self, state: &State, size: usize) -> bool {
        state.count >= self.limits.max_items
            || state.bytes + self.in_flight() + size > self.limits.max_bytes
    }

    /// Buffers `item`. Returns `false` when it was dropped (and counted).
    pub(crate) fn push(&self, item: PendingItem) -> bool {
        let size = item.estimated_size();
        if size > self.largest_item() {
            self.count_drop(&item.function_id, observe_codes::TOO_LARGE);
            return false;
        }
        let mut state = self.lock();
        if state.closed {
            state.count(&item.function_id, observe_codes::INACTIVE, 1);
            return false;
        }
        if self.full(&state, size) {
            state.count(&item.function_id, observe_codes::BUFFER_FULL, 1);
            return false;
        }
        let mut item = item;
        state.next_seq += 1;
        item.seq = state.next_seq;
        state.bytes += size;
        state.count += 1;
        state.items.push_back(item);
        let len = state.items.len();
        let first = state.oldest.is_none();
        if first {
            state.oldest = Some(Instant::now());
        }
        drop(state);
        if first || len == self.limits.batch_items {
            self.wake.notify_one();
        }
        true
    }

    pub(crate) fn count_drop(&self, function_id: &str, reason: &'static str) {
        self.lock().count(function_id, reason, 1);
    }

    /// Counts lost items, one per function id listed.
    pub(crate) fn count_items(&self, functions: &[String], reason: &'static str) {
        let mut state = self.lock();
        for function_id in functions {
            state.count(function_id, reason, 1);
        }
    }

    /// Counts the items of a frame that is lost, by reading it back. An
    /// unreadable frame (never produced by this SDK) counts once under `*`.
    pub(crate) fn count_frame(&self, frame: &str, reason: &'static str) {
        match serde_json::from_str::<FrameIn>(frame) {
            Ok(frame) => {
                let mut state = self.lock();
                for item in frame.items {
                    state.count(&item.function_id, reason, 1);
                }
            }
            Err(_) => self.count_drop("*", reason),
        }
    }

    /// Puts the items of report frames flushed on a connection that dropped
    /// back in the buffer, each frame with the seqs its lease carried: they
    /// wait for their function's confirmation on the next connection like
    /// any other item, in push order with everything buffered or parked
    /// since (see [`merge_in`]).
    ///
    /// A full buffer drops the newest items, as it always does: buffered
    /// ones newer than a requeued item make room for it, a requeued item
    /// that still does not fit is dropped. Each counts as `BUFFER_FULL`.
    pub(crate) fn requeue_frames<'a>(
        &self,
        frames: impl IntoIterator<Item = (&'a str, &'a [u64])>,
    ) {
        let mut items = Vec::new();
        let mut unreadable = 0;
        for (frame, seqs) in frames {
            match serde_json::from_str::<FrameIn>(frame) {
                Ok(frame) => {
                    let known = seqs.len() == frame.items.len();
                    items.extend(frame.items.into_iter().enumerate().map(|(i, item)| {
                        let mut item = PendingItem::from_observed(item);
                        if known {
                            item.seq = seqs[i];
                        }
                        item
                    }));
                }
                Err(_) => unreadable += 1,
            }
        }
        // Frames go out in flush order, which is not push order across
        // functions (a function confirmed late sends older items later).
        items.sort_by_key(|item| item.seq);
        let mut state = self.lock();
        if unreadable > 0 {
            state.count("*", observe_codes::INACTIVE, unreadable);
        }
        if state.closed {
            for item in items {
                state.count(&item.function_id, observe_codes::INACTIVE, 1);
            }
            return;
        }
        if items.is_empty() {
            return;
        }
        let mut requeued = Vec::with_capacity(items.len());
        for item in items {
            let size = item.estimated_size();
            // make room by dropping the buffered items newer than this one
            while self.full(&state, size)
                && state
                    .items
                    .back()
                    .is_some_and(|newest| newest.seq > item.seq)
            {
                let Some(newest) = state.items.pop_back() else {
                    break;
                };
                state.bytes = state.bytes.saturating_sub(newest.estimated_size());
                state.count -= 1;
                state.count(&newest.function_id, observe_codes::BUFFER_FULL, 1);
            }
            if self.full(&state, size) {
                state.count(&item.function_id, observe_codes::BUFFER_FULL, 1);
                continue;
            }
            state.bytes += size;
            state.count += 1;
            requeued.push(item);
        }
        let any = !requeued.is_empty();
        merge_in(&mut state.items, requeued);
        if any && state.oldest.is_none() {
            state.oldest = Some(Instant::now());
        }
    }

    /// The worker stopped: counts every item still buffered or parked as
    /// `INACTIVE`, and every item pushed or requeued from now on. A flush
    /// running meanwhile counts what it took the same way.
    pub(crate) fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        state.oldest = None;
        let items: Vec<PendingItem> = state.items.drain(..).collect();
        for item in items {
            state.bytes = state.bytes.saturating_sub(item.estimated_size());
            state.count -= 1;
            state.count(&item.function_id, observe_codes::INACTIVE, 1);
        }
        let parked: Vec<(String, (Instant, Vec<PendingItem>))> = state.parked.drain().collect();
        for (function_id, (_, items)) in parked {
            Self::release(&mut state, &function_id, items);
        }
    }

    /// Items buffered, parked ones included.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().count
    }

    /// When the next flush is due, if anything is waiting for one.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        let state = self.lock();
        if state.items.len() >= self.limits.batch_items {
            return Some(Instant::now());
        }
        state.oldest.map(|oldest| oldest + LINGER)
    }

    /// Moves the items parked for `function_id` back in line, after the
    /// engine confirmed it on the current connection.
    pub(crate) fn unpark(&self, function_id: &str) {
        let mut state = self.lock();
        state.record(function_id, Fate::Unparked);
        let Some((_, parked)) = state.parked.remove(function_id) else {
            return;
        };
        // in push order with what is waiting; parked items are usually the
        // oldest, a prepend
        merge_in(&mut state.items, parked);
        if state.oldest.is_none() {
            state.oldest = Some(Instant::now());
        }
        drop(state);
        self.wake.notify_one();
    }

    /// Drops the items buffered for a function that is gone or cannot be
    /// confirmed (unregistered, `FUNCTION_NAMESPACE_CONFLICT`), parked or
    /// not, as `INACTIVE`.
    pub(crate) fn discard(&self, function_id: &str) {
        let mut state = self.lock();
        state.record(function_id, Fate::Released);
        let mut dropped: Vec<PendingItem> = state
            .parked
            .remove(function_id)
            .map(|(_, items)| items)
            .unwrap_or_default();
        let (gone, kept): (Vec<PendingItem>, Vec<PendingItem>) = state
            .items
            .drain(..)
            .partition(|item| item.function_id == function_id);
        state.items = kept.into();
        dropped.extend(gone);
        Self::release(&mut state, function_id, dropped);
    }

    /// Drops parked items whose function was not confirmed within `max_age`
    /// (the engine never acknowledged it on this connection), as `INACTIVE`.
    /// Functions `confirmed` reports as confirmed on the current connection
    /// are kept, however long their hooks run.
    pub(crate) fn expire_parked(
        &self,
        max_age: std::time::Duration,
        confirmed: impl Fn(&str) -> bool,
    ) {
        let old: Vec<String> = self
            .lock()
            .parked
            .iter()
            .filter(|(_, (since, _))| since.elapsed() >= max_age)
            .map(|(id, _)| id.clone())
            .collect();
        // Asked outside the lock: `confirmed` looks the function up.
        let expired: Vec<String> = old.into_iter().filter(|id| !confirmed(id)).collect();
        let mut state = self.lock();
        for function_id in expired {
            // Still parked: not unparked meanwhile.
            if let Some((_, items)) = state.parked.remove(&function_id) {
                state.record(&function_id, Fate::Released);
                Self::release(&mut state, &function_id, items);
            }
        }
    }

    fn release(state: &mut State, function_id: &str, items: Vec<PendingItem>) {
        if items.is_empty() {
            return;
        }
        let bytes: usize = items.iter().map(PendingItem::estimated_size).sum();
        state.bytes = state.bytes.saturating_sub(bytes);
        state.count -= items.len();
        state.count(function_id, observe_codes::INACTIVE, items.len() as u64);
    }

    /// Packs `items` into frames of at most `batch_items` items and
    /// `frame_bytes` bytes, in order: `(json, function of each item, seq of
    /// each item)`. An item larger than a frame is dropped and listed apart.
    fn pack(
        &self,
        items: Vec<PendingItem>,
        v2: bool,
    ) -> (Vec<(String, Vec<String>, Vec<u64>)>, Vec<String>) {
        let largest = self.largest_item();
        let mut frames: Vec<(String, Vec<String>, Vec<u64>)> = Vec::new();
        let mut too_large = Vec::new();
        let mut frame = String::new();
        let mut functions = Vec::new();
        let mut seqs = Vec::new();
        for item in items {
            let json = item.to_json(v2);
            if json.len() > largest {
                too_large.push(item.function_id);
                continue;
            }
            let count = functions.len();
            let fits = frame.len() + 1 + json.len() + FRAME_SUFFIX.len() <= self.limits.frame_bytes;
            if count > 0 && (count == self.limits.batch_items || !fits) {
                frame.push_str(FRAME_SUFFIX);
                frames.push((
                    std::mem::take(&mut frame),
                    std::mem::take(&mut functions),
                    std::mem::take(&mut seqs),
                ));
            }
            if functions.is_empty() {
                frame.push_str(FRAME_PREFIX);
            } else {
                frame.push(',');
            }
            frame.push_str(&json);
            functions.push(item.function_id);
            seqs.push(item.seq);
        }
        if !functions.is_empty() {
            frame.push_str(FRAME_SUFFIX);
            frames.push((frame, functions, seqs));
        }
        (frames, too_large)
    }

    /// `items` as frames to send now, outside the buffer: the report items of
    /// one routed call, `(json, function of each item)`. Items larger than a
    /// frame are counted as `TOO_LARGE`.
    pub(crate) fn frames_of(
        &self,
        items: Vec<PendingItem>,
        v2: bool,
    ) -> Vec<(String, Vec<String>)> {
        let (frames, too_large) = self.pack(items, v2);
        if !too_large.is_empty() {
            let mut state = self.lock();
            for function_id in too_large {
                state.count(&function_id, observe_codes::TOO_LARGE, 1);
            }
        }
        frames
            .into_iter()
            .map(|(json, functions, _)| (json, functions))
            .collect()
    }

    /// Takes every waiting item and returns those `gate` lets through as
    /// frames of at most `batch_items` items and `frame_bytes` bytes, each
    /// holding its bytes against the bound until it is written. Held items
    /// are parked per function; dropped ones are counted as `INACTIVE`.
    pub(crate) fn take_report_frames(
        &self,
        gate: impl Fn(&str) -> Gate,
        v2: bool,
    ) -> Vec<ReportFrame> {
        let (taken, since): (Vec<PendingItem>, u64) = {
            let mut state = self.lock();
            state.oldest = None;
            if state.items.is_empty() {
                return Vec::new();
            }
            // what happened before this flush is already reflected in `items`
            // and `parked`; record what happens until it is done
            state.fates.clear();
            state.taking = true;
            (state.items.drain(..).collect(), state.fate_seq)
        };

        // Gates are asked outside the lock (they look functions up); what an
        // `unpark` or `discard` did meanwhile is re-checked before parking.
        let mut gates: HashMap<String, Gate> = HashMap::new();
        let mut sendable = Vec::with_capacity(taken.len());
        let mut parked = Vec::new();
        let mut released = (0usize, 0usize);
        let mut dropped: Vec<(String, &'static str)> = Vec::new();
        for item in taken {
            let gate = *gates
                .entry(item.function_id.clone())
                .or_insert_with(|| gate(&item.function_id));
            match gate {
                Gate::Send => {
                    released.0 += 1;
                    released.1 += item.estimated_size();
                    sendable.push(item);
                }
                Gate::Hold => parked.push(item),
                Gate::Drop => {
                    released.0 += 1;
                    released.1 += item.estimated_size();
                    dropped.push((item.function_id, observe_codes::INACTIVE));
                }
            }
        }

        let (frames, too_large) = self.pack(sendable, v2);
        dropped.extend(
            too_large
                .into_iter()
                .map(|function_id| (function_id, observe_codes::TOO_LARGE)),
        );

        // The items leave the buffer and their frames enter the in-flight
        // count under one lock: a `push` never sees the bytes in neither.
        let mut state = self.lock();
        state.count -= released.0;
        state.bytes = state.bytes.saturating_sub(released.1);
        let mut requeued = Vec::new();
        for item in parked {
            let fates = state
                .fates
                .get(&item.function_id)
                .copied()
                .unwrap_or_default();
            // A release while this flush ran wins over a later unpark: these
            // items were taken before it. A worker that stopped meanwhile
            // sends nothing more.
            let fate = if state.closed || fates.released > since {
                Some(Fate::Released)
            } else if fates.unparked > since {
                Some(Fate::Unparked)
            } else {
                None
            };
            match fate {
                // confirmed while this flush ran: back in line
                Some(Fate::Unparked) => requeued.push(item),
                // released while this flush ran
                Some(Fate::Released) => {
                    let function_id = item.function_id.clone();
                    Self::release(&mut state, &function_id, vec![item]);
                }
                None => insert_in_order(
                    &mut state
                        .parked
                        .entry(item.function_id.clone())
                        .or_insert_with(|| (Instant::now(), Vec::new()))
                        .1,
                    item,
                ),
            }
        }
        state.fates.clear();
        state.taking = false;
        let wake = !requeued.is_empty();
        if wake {
            merge_in(&mut state.items, requeued);
            if state.oldest.is_none() {
                state.oldest = Some(Instant::now());
            }
        }
        for (function_id, reason) in dropped {
            state.count(&function_id, reason, 1);
        }
        let frames = frames
            .into_iter()
            .map(|(json, functions, seqs)| {
                let mut lease = self.lease(json.len());
                lease.seqs = seqs;
                ReportFrame {
                    json,
                    lease,
                    functions,
                }
            })
            .collect();
        drop(state);
        if wake {
            self.wake.notify_one();
        }
        frames
    }

    /// [`take_report_frames`](Self::take_report_frames) for an `observe.v2`
    /// engine, the JSON only.
    #[cfg(test)]
    pub(crate) fn take_frames(&self, gate: impl Fn(&str) -> Gate) -> Vec<String> {
        self.take_report_frames(gate, true)
            .into_iter()
            .map(|frame| frame.json)
            .collect()
    }

    /// Takes the drop counters: `(function_id, reason, count)`.
    pub(crate) fn take_drops(&self) -> Vec<(String, &'static str, u64)> {
        self.lock()
            .drops
            .drain()
            .map(|((function_id, reason), count)| (function_id, reason, count))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json, value::to_raw_value};

    use super::*;

    fn raw(value: Value) -> Option<Box<RawValue>> {
        Some(to_raw_value(&value).unwrap())
    }

    fn item(function_id: &str, matched: &[&str], input: Value) -> PendingItem {
        PendingItem {
            matched: matched.iter().map(|s| s.to_string()).collect(),
            input: raw(input),
            mask: CaptureMask {
                input: true,
                ..CaptureMask::default()
            },
            ..PendingItem::test(function_id)
        }
    }

    /// Flushes what can go, as frames a dropped connection leaves behind:
    /// their leases released, their seqs kept.
    fn sent(reporter: &Reporter) -> Vec<(String, Vec<u64>)> {
        reporter
            .take_report_frames(|_| Gate::Send, true)
            .into_iter()
            .map(|mut frame| {
                (
                    std::mem::take(&mut frame.json),
                    std::mem::take(&mut frame.lease.seqs),
                )
            })
            .collect()
    }

    fn requeue(reporter: &Reporter, frames: &[(String, Vec<u64>)]) {
        reporter.requeue_frames(
            frames
                .iter()
                .map(|(json, seqs)| (json.as_str(), seqs.as_slice())),
        );
    }

    fn items_of(frame: &str) -> Vec<Value> {
        let value: Value = serde_json::from_str(frame).unwrap();
        assert_eq!(value["type"], "invocationsobserved");
        value["items"].as_array().unwrap().clone()
    }

    #[test]
    fn frames_carry_the_masked_parts_only() {
        let reporter = Reporter::default();
        let mut it = item("f", &["o1"], json!({ "a": 1 }));
        it.output = raw(json!(null));
        it.caller = Some(Caller::default());
        assert!(reporter.push(it));
        let frames = reporter.take_frames(|_| Gate::Send);
        assert_eq!(frames.len(), 1);
        let items = items_of(&frames[0]);
        assert_eq!(items[0]["input"], json!({ "a": 1 }));
        assert!(items[0].get("output").is_none(), "output is not captured");
        assert!(items[0].get("caller").is_none());
        assert_eq!(items[0]["observe_depth"], 0);
        assert_eq!(reporter.len(), 0);
    }

    #[test]
    fn a_full_buffer_drops_the_newest_and_counts_it() {
        let reporter = Reporter::with_limits(Limits {
            max_items: 3,
            ..Limits::default()
        });
        for i in 0..5 {
            let kept = reporter.push(item("f", &["o"], json!(i)));
            assert_eq!(kept, i < 3);
        }
        let drops = reporter.take_drops();
        assert_eq!(drops, vec![("f".to_string(), "BUFFER_FULL", 2)]);
        let frames = reporter.take_frames(|_| Gate::Send);
        let inputs: Vec<Value> = items_of(&frames[0])
            .iter()
            .map(|i| i["input"].clone())
            .collect();
        assert_eq!(
            inputs,
            vec![json!(0), json!(1), json!(2)],
            "the oldest are kept"
        );
    }

    #[test]
    fn the_byte_bound_drops_the_newest() {
        let reporter = Reporter::with_limits(Limits {
            max_bytes: 2_000,
            ..Limits::default()
        });
        assert!(reporter.push(item("f", &["o"], json!("x".repeat(1_000)))));
        assert!(!reporter.push(item("f", &["o"], json!("y".repeat(1_000)))));
        assert!(
            reporter.push(item("f", &["o"], json!(1))),
            "small items still fit"
        );
        assert_eq!(
            reporter.take_drops(),
            vec![("f".to_string(), "BUFFER_FULL", 1)]
        );
    }

    #[test]
    fn an_item_larger_than_a_frame_is_too_large() {
        let reporter = Reporter::with_limits(Limits {
            frame_bytes: 1_000,
            ..Limits::default()
        });
        assert!(!reporter.push(item("f", &["o"], json!("x".repeat(2_000)))));
        assert_eq!(
            reporter.take_drops(),
            vec![("f".to_string(), "TOO_LARGE", 1)]
        );
        assert_eq!(reporter.len(), 0);
    }

    #[test]
    fn an_item_larger_than_the_buffer_is_too_large() {
        let reporter = Reporter::with_limits(Limits {
            max_bytes: 1_000,
            ..Limits::default()
        });
        assert!(!reporter.push(item("f", &["o"], json!("x".repeat(2_000)))));
        assert_eq!(
            reporter.take_drops(),
            vec![("f".to_string(), "TOO_LARGE", 1)]
        );
    }

    #[test]
    fn frames_hold_at_most_a_batch_and_stay_under_the_frame_limit() {
        let reporter = Reporter::with_limits(Limits {
            frame_bytes: 4_096,
            ..Limits::default()
        });
        for i in 0..600 {
            reporter.push(item("f", &["o"], json!(i)));
        }
        let frames = reporter.take_frames(|_| Gate::Send);
        let total: usize = frames.iter().map(|f| items_of(f).len()).sum();
        assert_eq!(total, 600);
        for frame in &frames {
            assert!(frame.len() <= 4_096, "frame of {} bytes", frame.len());
            assert!(items_of(frame).len() <= BATCH_ITEMS);
        }

        let reporter = Reporter::default();
        for i in 0..600 {
            reporter.push(item("f", &["o"], json!(i)));
        }
        let sizes: Vec<usize> = reporter
            .take_frames(|_| Gate::Send)
            .iter()
            .map(|f| items_of(f).len())
            .collect();
        assert_eq!(sizes, vec![256, 256, 88]);
    }

    /// An item whose output is its input carries the value once to an
    /// `observe.v2` engine, and as both `input` and `output` to an older one.
    #[test]
    fn output_is_input_carries_the_value_once_to_a_v2_engine() {
        let event = json!({ "table": "invoices", "op": "update" });
        let source = || PendingItem {
            output_is_input: true,
            mask: CaptureMask {
                input: true,
                output: true,
                ..CaptureMask::default()
            },
            ..item("db::on-row", &["o"], event.clone())
        };
        let once = source();
        let size = once.estimated_size();
        assert!(size < 2 * once.input.as_ref().unwrap().get().len() + ITEM_OVERHEAD);
        let v2: Value = serde_json::from_str(&once.to_json(true)).unwrap();
        assert_eq!(v2["input"], event);
        assert_eq!(v2["output_is_input"], true);
        assert!(v2.get("output").is_none());

        let v1: Value = serde_json::from_str(&source().to_json(false)).unwrap();
        assert_eq!(v1["input"], event);
        assert_eq!(v1["output"], event);
        assert!(v1.get("output_is_input").is_none());

        // read back after a dropped connection, it keeps the flag
        let reporter = Reporter::default();
        reporter.push(source());
        let frames = sent(&reporter);
        requeue(&reporter, &frames);
        let items = items_of(&reporter.take_frames(|_| Gate::Send)[0]);
        assert_eq!(items[0]["output_is_input"], true);
        assert!(items[0].get("output").is_none());
    }

    /// A routed call's items go out as frames of their own, outside the
    /// buffer; one larger than a frame is `TOO_LARGE`.
    #[test]
    fn routed_items_are_framed_outside_the_buffer() {
        let reporter = Reporter::with_limits(Limits {
            frame_bytes: 2_000,
            ..Limits::default()
        });
        let frames = reporter.frames_of(
            vec![
                item("f", &["a"], json!(1)),
                item("f", &["b"], json!("x".repeat(3_000))),
                item("f", &["c"], json!(2)),
            ],
            true,
        );
        assert_eq!(frames.len(), 1);
        let (json, functions) = &frames[0];
        assert_eq!(functions, &vec!["f".to_string(), "f".to_string()]);
        let matched: Vec<Value> = items_of(json)
            .iter()
            .map(|i| i["matched"].clone())
            .collect();
        assert_eq!(matched, vec![json!(["a"]), json!(["c"])]);
        assert_eq!(reporter.len(), 0, "nothing buffered");
        assert_eq!(
            reporter.take_drops(),
            vec![("f".to_string(), "TOO_LARGE", 1)]
        );
    }

    #[test]
    fn held_items_are_parked_until_their_function_is_confirmed() {
        let reporter = Reporter::default();
        reporter.push(item("held", &["o"], json!(1)));
        reporter.push(item("sent", &["o"], json!(2)));
        reporter.push(item("gone", &["o"], json!(3)));
        reporter.push(item("held", &["o"], json!(4)));
        let gate = |id: &str| match id {
            "held" => Gate::Hold,
            "sent" => Gate::Send,
            _ => Gate::Drop,
        };
        let frames = reporter.take_frames(gate);
        assert_eq!(items_of(&frames[0]).len(), 1);
        assert_eq!(reporter.len(), 2, "parked items still count");
        assert!(
            reporter.deadline().is_none(),
            "parked items do not wake the loop"
        );
        assert_eq!(
            reporter.take_drops(),
            vec![("gone".to_string(), "INACTIVE", 1)]
        );

        reporter.push(item("sent", &["o"], json!(5)));
        reporter.unpark("held");
        let inputs: Vec<Value> = items_of(&reporter.take_frames(|_| Gate::Send)[0])
            .iter()
            .map(|i| i["input"].clone())
            .collect();
        assert_eq!(inputs, vec![json!(1), json!(4), json!(5)]);
        assert_eq!(reporter.len(), 0);
    }

    #[test]
    fn frames_not_written_yet_count_against_the_bound() {
        let reporter = Reporter::with_limits(Limits {
            max_bytes: 4_000,
            ..Limits::default()
        });
        let lease = reporter.lease(3_000);
        assert!(!reporter.push(item("f", &["o"], json!("x".repeat(1_000)))));
        assert_eq!(
            reporter.take_drops(),
            vec![("f".to_string(), "BUFFER_FULL", 1)]
        );
        drop(lease);
        assert_eq!(reporter.in_flight(), 0);
        assert!(reporter.push(item("f", &["o"], json!("x".repeat(1_000)))));
    }

    #[test]
    fn parked_items_expire_and_conflicted_functions_are_discarded() {
        let reporter = Reporter::default();
        reporter.push(item("stale", &["o"], json!(1)));
        reporter.push(item("conflict", &["o"], json!(2)));
        reporter.push(item("confirmed", &["o"], json!(4)));
        assert!(reporter.take_frames(|_| Gate::Hold).is_empty());
        reporter.push(item("conflict", &["o"], json!(3)));
        reporter.discard("conflict");
        assert_eq!(reporter.len(), 2, "both of conflict's items, parked or not");
        // a function confirmed on this connection keeps its reports, however
        // long its hooks run
        reporter.expire_parked(std::time::Duration::ZERO, |id| id == "confirmed");
        assert_eq!(reporter.len(), 1);
        reporter.unpark("confirmed");
        assert_eq!(reporter.take_frames(|_| Gate::Send).len(), 1);
        assert_eq!(reporter.len(), 0);
        let mut drops = reporter.take_drops();
        drops.sort();
        assert_eq!(
            drops,
            vec![
                ("conflict".to_string(), "INACTIVE", 2),
                ("stale".to_string(), "INACTIVE", 1)
            ]
        );
    }

    /// A `discard` that runs while a flush has the items out of the queue
    /// still releases them.
    #[test]
    fn a_discard_during_a_flush_releases_the_items_it_took() {
        let reporter = Reporter::default();
        for i in 0..10 {
            reporter.push(item("f", &["o"], json!(i)));
        }
        let frames = reporter.take_report_frames(
            |id| {
                reporter.discard(id);
                Gate::Hold
            },
            true,
        );
        assert!(frames.is_empty());
        assert_eq!(reporter.len(), 0, "nothing left counted");
        assert_eq!(
            reporter.take_drops(),
            vec![("f".to_string(), "INACTIVE", 10)]
        );
    }

    /// An `unpark` that runs while a flush has the items out of the queue
    /// puts them back in line instead of stranding them.
    #[test]
    fn an_unpark_during_a_flush_does_not_strand_the_items() {
        let reporter = Reporter::default();
        reporter.push(item("f", &["o"], json!(1)));
        let frames = reporter.take_report_frames(
            |id| {
                reporter.unpark(id);
                Gate::Hold
            },
            true,
        );
        assert!(frames.is_empty());
        assert!(reporter.deadline().is_some(), "back in line");
        let items = items_of(&reporter.take_frames(|_| Gate::Send)[0]);
        assert_eq!(items.len(), 1);
        assert_eq!(reporter.len(), 0);
    }

    /// A release then an unpark during one flush: the items taken before
    /// the release are dropped, not requeued.
    #[test]
    fn a_release_then_an_unpark_during_a_flush_drops_the_items() {
        let reporter = Reporter::default();
        for i in 0..3 {
            reporter.push(item("f", &["o"], json!(i)));
        }
        let frames = reporter.take_report_frames(
            |id| {
                reporter.discard(id);
                reporter.unpark(id);
                Gate::Hold
            },
            true,
        );
        assert!(frames.is_empty());
        assert_eq!(reporter.len(), 0);
        assert_eq!(
            reporter.take_drops(),
            vec![("f".to_string(), "INACTIVE", 3)]
        );
    }

    fn inputs(reporter: &Reporter) -> Vec<Value> {
        reporter
            .take_frames(|_| Gate::Send)
            .iter()
            .flat_map(|frame| items_of(frame))
            .map(|i| i["input"].clone())
            .collect()
    }

    /// A frame read back after a reconnect is older than an item of the same
    /// function parked during the reconnect: unparking keeps push order.
    #[test]
    fn requeued_items_stay_ahead_of_newer_parked_ones() {
        let reporter = Reporter::default();
        reporter.push(item("f", &["o"], json!("old")));
        let flushed = sent(&reporter);
        reporter.push(item("f", &["o"], json!("new")));
        assert!(reporter.take_frames(|_| Gate::Hold).is_empty());
        requeue(&reporter, &flushed);
        reporter.unpark("f");
        assert_eq!(inputs(&reporter), vec![json!("old"), json!("new")]);
    }

    /// The same, with the frame read back while a flush holds the newer item.
    #[test]
    fn a_requeue_during_a_flush_keeps_push_order() {
        let reporter = Reporter::default();
        reporter.push(item("f", &["o"], json!("old")));
        let flushed = sent(&reporter);
        reporter.push(item("f", &["o"], json!("new")));
        let frames = reporter.take_report_frames(
            |_| {
                requeue(&reporter, &flushed);
                Gate::Hold
            },
            true,
        );
        assert!(frames.is_empty());
        reporter.unpark("f");
        assert_eq!(inputs(&reporter), vec![json!("old"), json!("new")]);

        // and when the function is confirmed during that flush
        reporter.push(item("g", &["o"], json!("old")));
        let flushed = sent(&reporter);
        reporter.push(item("g", &["o"], json!("new")));
        let frames = reporter.take_report_frames(
            |id| {
                requeue(&reporter, &flushed);
                reporter.unpark(id);
                Gate::Hold
            },
            true,
        );
        assert!(frames.is_empty());
        assert_eq!(inputs(&reporter), vec![json!("old"), json!("new")]);
    }

    #[test]
    fn fates_are_recorded_only_during_a_flush() {
        let reporter = Reporter::default();
        for i in 0..1_000 {
            reporter.unpark(&format!("f{i}"));
            reporter.discard(&format!("f{i}"));
        }
        assert!(reporter.lock().fates.is_empty(), "no flush running");
        reporter.push(item("g", &["o"], json!(1)));
        reporter.take_frames(|id| {
            reporter.unpark(id);
            Gate::Send
        });
        assert!(reporter.lock().fates.is_empty(), "cleared after the flush");
    }

    #[test]
    fn drop_counters_are_bounded() {
        let reporter = Reporter::default();
        for i in 0..(MAX_DROP_KEYS + 10) {
            reporter.count_drop(&format!("f{i}"), observe_codes::INACTIVE);
        }
        let drops = reporter.take_drops();
        assert_eq!(drops.len(), MAX_DROP_KEYS + 1);
        assert!(drops.contains(&("*".to_string(), "INACTIVE", 10)));
    }

    /// Requeued items are older than what was buffered since: a full buffer
    /// drops the newest buffered ones to make room.
    #[test]
    fn a_full_buffer_drops_the_newest_when_items_are_requeued() {
        let reporter = Reporter::with_limits(Limits {
            max_items: 3,
            ..Limits::default()
        });
        reporter.push(item("old", &["o"], json!(1)));
        let frames = sent(&reporter);
        for i in 0..3 {
            reporter.push(item("new", &["o"], json!(i)));
        }
        requeue(&reporter, &frames);
        reporter.requeue_frames([("not json", &[][..])]);
        assert_eq!(reporter.len(), 3);
        let inputs: Vec<Value> = items_of(&reporter.take_frames(|_| Gate::Send)[0])
            .iter()
            .map(|i| i["input"].clone())
            .collect();
        assert_eq!(
            inputs,
            vec![json!(1), json!(0), json!(1)],
            "the newest went"
        );
        let mut drops = reporter.take_drops();
        drops.sort();
        assert_eq!(
            drops,
            vec![
                ("*".to_string(), "INACTIVE", 1),
                ("new".to_string(), "BUFFER_FULL", 1)
            ]
        );
    }

    /// Report frames that were flushed on a connection that dropped come back
    /// as items, ahead of newer ones, and wait for confirmation again.
    #[test]
    fn frames_of_a_dropped_connection_come_back_as_items() {
        let reporter = Reporter::default();
        reporter.push(item("f", &["o"], json!("old")));
        let frames = sent(&reporter);
        reporter.push(item("f", &["o"], json!("new")));
        requeue(&reporter, &frames);
        assert_eq!(reporter.len(), 2);
        assert!(
            reporter.take_frames(|_| Gate::Hold).is_empty(),
            "held again"
        );
        reporter.unpark("f");
        let inputs: Vec<Value> = items_of(&reporter.take_frames(|_| Gate::Send)[0])
            .iter()
            .map(|i| i["input"].clone())
            .collect();
        assert_eq!(inputs, vec![json!("old"), json!("new")]);
    }

    #[test]
    fn parked_items_of_a_gone_function_are_inactive() {
        let reporter = Reporter::default();
        reporter.push(item("held", &["o"], json!(1)));
        assert!(reporter.take_frames(|_| Gate::Hold).is_empty());
        reporter.discard("held");
        assert_eq!(reporter.len(), 0);
        assert_eq!(
            reporter.take_drops(),
            vec![("held".to_string(), "INACTIVE", 1)]
        );
    }

    fn seqs(items: &VecDeque<PendingItem>) -> Vec<u64> {
        items.iter().map(|item| item.seq).collect()
    }

    fn with_seqs(seqs: &[u64]) -> Vec<PendingItem> {
        seqs.iter()
            .map(|&seq| PendingItem {
                seq,
                ..PendingItem::test("f")
            })
            .collect()
    }

    #[test]
    fn merging_keeps_seq_order() {
        let mut items: VecDeque<PendingItem> = with_seqs(&[2, 4, 6]).into();
        merge_in(&mut items, with_seqs(&[0, 1]));
        assert_eq!(seqs(&items), vec![0, 1, 2, 4, 6], "all older: prepended");
        merge_in(&mut items, with_seqs(&[7, 8]));
        assert_eq!(
            seqs(&items),
            vec![0, 1, 2, 4, 6, 7, 8],
            "all newer: appended"
        );
        merge_in(&mut items, with_seqs(&[3, 5, 9]));
        assert_eq!(seqs(&items), vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        merge_in(&mut items, Vec::new());
        assert_eq!(items.len(), 10);
        let mut empty = VecDeque::new();
        merge_in(&mut empty, with_seqs(&[1, 2]));
        assert_eq!(seqs(&empty), vec![1, 2]);
    }

    /// Items read back after a drop keep their place in a function's parked
    /// list, behind none of the newer items parked meanwhile.
    #[test]
    fn requeued_items_are_parked_in_push_order() {
        let reporter = Reporter::default();
        reporter.push(item("f", &["o"], json!("old")));
        let flushed = sent(&reporter);
        reporter.push(item("f", &["o"], json!("new")));
        assert!(reporter.take_frames(|_| Gate::Hold).is_empty());
        requeue(&reporter, &flushed);
        assert!(reporter.take_frames(|_| Gate::Hold).is_empty());
        reporter.unpark("f");
        assert_eq!(inputs(&reporter), vec![json!("old"), json!("new")]);
    }

    /// A stopped worker counts what it still holds, and whatever comes
    /// after, as `INACTIVE`.
    #[test]
    fn closing_counts_every_item_left() {
        let reporter = Reporter::default();
        reporter.push(item("parked", &["o"], json!(1)));
        assert!(reporter.take_frames(|_| Gate::Hold).is_empty());
        reporter.push(item("buffered", &["o"], json!(1)));
        let flushed = sent(&reporter);
        reporter.push(item("buffered", &["o"], json!(2)));
        reporter.close();
        assert_eq!(reporter.len(), 0);
        assert!(!reporter.push(item("late", &["o"], json!(1))));
        requeue(&reporter, &flushed);
        assert!(reporter.take_frames(|_| Gate::Send).is_empty());
        assert_eq!(reporter.deadline(), None);
        let mut drops = reporter.take_drops();
        drops.sort();
        assert_eq!(
            drops,
            vec![
                ("buffered".to_string(), "INACTIVE", 2),
                ("late".to_string(), "INACTIVE", 1),
                ("parked".to_string(), "INACTIVE", 1),
            ]
        );

        // a flush running when the worker stops parks nothing
        let reporter = Reporter::default();
        reporter.push(item("f", &["o"], json!(1)));
        reporter.push(item("g", &["o"], json!(1)));
        let frames = reporter.take_report_frames(
            |id| {
                reporter.close();
                if id == "g" { Gate::Send } else { Gate::Hold }
            },
            true,
        );
        assert_eq!(frames.len(), 1, "the caller counts what it cannot send");
        assert_eq!(reporter.len(), 0);
        assert_eq!(
            reporter.take_drops(),
            vec![("f".to_string(), "INACTIVE", 1)]
        );
    }

    #[test]
    fn the_first_item_and_a_full_batch_wake_the_flusher() {
        let reporter = Reporter::default();
        assert!(reporter.deadline().is_none());
        reporter.push(item("f", &["o"], json!(0)));
        let deadline = reporter.deadline().unwrap();
        assert!(deadline <= Instant::now() + LINGER);
        for i in 1..BATCH_ITEMS {
            reporter.push(item("f", &["o"], json!(i)));
        }
        assert!(reporter.deadline().unwrap() <= Instant::now());
    }

    #[test]
    fn the_item_encoding_matches_the_fixture_shape() {
        let fixtures = [
            include_str!("../../../../fixtures/protocol/observe/invocationsobserved.json"),
            include_str!(
                "../../../../fixtures/protocol/observe/invocationsobserved-event-source.json"
            ),
            include_str!("../../../../fixtures/protocol/observe/invocationsobserved-routed.json"),
        ];
        let expected: Vec<Value> = fixtures
            .iter()
            .flat_map(|fixture| {
                serde_json::from_str::<Value>(fixture).unwrap()["items"]
                    .as_array()
                    .unwrap()
                    .clone()
            })
            .collect();
        for expected in &expected {
            let typed: crate::protocol::ObservedItem =
                serde_json::from_value(expected.clone()).unwrap();
            let pending = PendingItem {
                function_id: typed.function_id.clone(),
                namespace: typed.namespace.clone(),
                invocation_id: typed.invocation_id.clone(),
                matched: typed.matched.clone(),
                status: typed.status.clone(),
                input: typed.input.as_ref().map(|v| to_raw_value(v).unwrap()),
                output: typed.output.as_ref().map(|v| to_raw_value(v).unwrap()),
                error: typed.error.as_ref().map(|v| to_raw_value(v).unwrap()),
                caller: typed.caller.clone(),
                metadata: typed.metadata.as_ref().map(|v| to_raw_value(v).unwrap()),
                started_at_ms: typed.started_at_ms,
                duration_ms: typed.duration_ms,
                observe_depth: typed.observe_depth,
                traceparent: typed.traceparent.clone(),
                baggage: typed.baggage.clone(),
                mask: CaptureMask {
                    input: true,
                    output: true,
                    error: true,
                    caller: true,
                    metadata: true,
                },
                output_is_input: typed.output_is_input,
                seq: 0,
            };
            let encoded: Value = serde_json::from_str(&pending.to_json(true)).unwrap();
            assert_eq!(&encoded, expected);
        }
    }
}
