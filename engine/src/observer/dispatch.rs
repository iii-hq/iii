// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The routed hook, reports, calls through an observer and activations
//! (protocol.md §3.2, §3.4; v2 §4.6).
//!
//! `InvocationHandler::handle_invocation_inner` asks
//! [`ObserverRegistry::route`] before running a handler: one atomic load (how
//! many functions the routed view routes) and, only when some function is
//! observed or hosted privately by a connection, a load of the routed view and
//! one hash lookup, then one atomic load of the function's active set when the
//! view holds the function. Unobserved, nothing is copied.
//! Observed, the hook keeps the parts of the invocation the active observers
//! need, and once the result is known [`ObserverRegistry::complete`] hands it
//! to an evaluator task, so the caller's reply never waits for filters or
//! activations. There is one evaluator per runtime worker thread, and a
//! function's completions always go to the same one, which handles them in the
//! order they completed: an observer watches one function, so its activations
//! stay in engine arrival order while different functions are evaluated in
//! parallel.
//!
//! Who matches a call's observers (v2 §8.2, decisions 2 and 55). The engine
//! evaluates the observers' `on` and filters of every routed call, with the
//! input it kept and the answer's result, through each function's
//! [`iii_filter::FilterIndex`]: about one lookup per indexed path, whatever
//! the number of observers. A host that declared `observe.v2` keeps what is
//! worker code: its answer (`invocationresult`) lists the observers its
//! function's match hook matched (`matched`), among which the engine
//! evaluates, or says that its deliver hook delivered the call
//! (`delivered`), and the engine activates nothing; an answer with neither
//! leaves every active observer to the engine's evaluation. Only the
//! function's host answers for its observers: an answer from another
//! `observe.v2` connection activates nothing (`REPORTER_NOT_OWNER`). A
//! routed call of such a host that ends without its answer (its caller
//! left, the engine could not send it, the host went away) activates
//! nothing: each of its observers counts a `CANCELLED` drop. The host
//! matches and reports its local calls itself (`invocationsobserved`), and
//! the engine activates the observers each report names.
//!
//! A matched event is shared, not copied, by the observers it activates: a
//! queued activation holds a pointer to it, and the target's argument is built
//! when the activation starts.

use std::{
    borrow::Cow,
    collections::HashSet,
    hash::{BuildHasher, RandomState},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use iii_filter::Doc;
use once_cell::race::OnceBox;
use opentelemetry::KeyValue;
use serde_json::{Map, Value, value::RawValue};
use tokio::sync::{mpsc, oneshot::error::RecvError};
use uuid::Uuid;

use super::{
    FnKey, ObserverRegistry,
    context::{ENGINE_CALLER_NAME, InvocationCtx},
    lifecycle::{
        CANCELLED, Deliver, INVALID_REPORT, Observer, PendingActivation, failure_reasons, lock,
        now_ms,
    },
    owner::{ActiveSet, Needs},
};
use crate::{
    invocation::{
        Dispatch,
        encoded::{EncodedJson, ObjectWriter},
    },
    protocol::{
        Activation, Caller, DroppedReportStats, ErrorBody, InvocationStatus, LocalFunctionStats,
        ObservedItem, ObservedItems, RawJson, observe_codes,
    },
    worker_connections::WorkerConnection,
};

/// An invocation of an observed function, from before its handler runs until
/// its result is known. Dropped without a result (the invocation was
/// cancelled: its caller gave up, e.g. an activation timeout), it counts a
/// `CANCELLED` drop for each observer that was waiting for it, since nobody
/// can know which of them would have matched.
pub struct ObservedCall {
    inner: Option<Tracked>,
}

struct Tracked {
    set: Arc<ActiveSet>,
    /// The function's host declared `observe.v2`: only its answer says how
    /// the call is delivered.
    host_delivers: bool,
    /// What the call keeps (see [`ActiveSet::needs`]).
    needs: Needs,
    /// The evaluator had room when the call began: what it needs was kept.
    admitted: bool,
    started: Instant,
    started_at_ms: u64,
    /// The input as the observers need it: the fields their filters read,
    /// when that is all they read of it, and otherwise all of it (see
    /// [`Payload::of`]).
    input: Option<Payload>,
    metadata: Option<Arc<Value>>,
    traceparent: Option<String>,
    baggage: Option<String>,
}

impl Drop for ObservedCall {
    fn drop(&mut self) {
        if let Some(cancelled) = self.inner.take() {
            for observer in &cancelled.set.observers {
                drop_event(observer, CANCELLED);
            }
        }
    }
}

/// Completions waiting for one evaluator. Past this, or past
/// [`EVALUATOR_BYTES`] of payloads waiting for all of them, a completion is
/// not evaluated (nor its payload copied): each active observer of the
/// function counts an `OVERFLOW` drop.
pub(crate) const EVALUATOR_QUEUE: usize = 65_536;

/// Approximate bytes of payloads waiting for the evaluators. A completion
/// whose payloads alone are larger never fits, however idle the evaluators:
/// a `TOO_LARGE` drop, not an `OVERFLOW`.
pub(crate) const EVALUATOR_BYTES: usize = 64 * 1024 * 1024;

/// Evaluators at most, whatever the number of runtime worker threads.
const MAX_EVALUATORS: usize = 16;

/// How often drop counts reach `iii.observer.dropped.total` (see
/// [`ObserverRegistry::export_drops`]).
pub(crate) const DROP_EXPORT_INTERVAL: Duration = Duration::from_secs(1);

/// How long a reporting connection waits for room in the queues its report
/// overflowed before the engine reads from it again (see
/// [`ObserverRegistry::wait_for_room`]).
pub(crate) const REPORT_BACKPRESSURE_WAIT: Duration = Duration::from_millis(250);

/// A hold of a reporting connection is followed by at least
/// `1 / REPORT_HOLD_PER_READ` of its length spent reading before the next one
/// (see [`ReportHold`]).
pub(crate) const REPORT_HOLD_PER_READ: u32 = 4;

/// When a reporting connection may be held again, kept by its reader (see
/// [`ObserverRegistry::wait_for_room`]). After a hold the engine reads the
/// connection for at least a quarter as long ([`REPORT_HOLD_PER_READ`]), and
/// a report that overflows meanwhile is dropped (`OVERFLOW`) without holding
/// it. So the connection is held at most four fifths of the time, and what it
/// sends besides reports (its calls, its answers to calls) waits at most
/// [`REPORT_BACKPRESSURE_WAIT`] plus five times the time the engine takes to
/// read the reports ahead of it, however long they keep overflowing. Reading
/// meanwhile means decoding reports only to drop them: reading half the time
/// cost a one-vCPU engine about a third of its activations in a report flood.
#[derive(Debug, Default)]
pub struct ReportHold {
    not_before: Option<tokio::time::Instant>,
}

/// How many of an `observe.v2` host's last answered routed calls
/// [`RoutedAnswers`] keeps: several report lingers' worth of answers (an SDK
/// flushes its report at least every 5 ms) at any rate one connection
/// answers.
pub(crate) const ROUTED_ANSWERS_KEPT: usize = 4096;

/// A free slot of [`RoutedAnswers`]' index.
const FREE: u16 = u16::MAX;

// The index never has more than 2^16 slots, so a 16-bit hash places an id
// in it and a slot holds any position in the window, none of them `FREE`.
const _: () = assert!((2 * ROUTED_ANSWERS_KEPT).next_power_of_two() <= 1 << 16);

/// The routed calls an `observe.v2` host answered last, by invocation id,
/// [`ROUTED_ANSWERS_KEPT`] at most, kept by its connection: a report item
/// naming one is dropped as `INVALID_REPORT` after the answer as well as
/// before it (decision 38).
///
/// Each id is held once, in a ring no larger than the window, and found
/// through an index of 2-byte positions in the ring: 22 bytes per kept call,
/// about 88 KiB once the host answered [`ROUTED_ANSWERS_KEPT`] calls however
/// many more it answers, and nothing before its first answer.
#[derive(Debug, Default)]
pub struct RoutedAnswers {
    kept: std::sync::Mutex<KeptIds>,
}

/// The window of [`RoutedAnswers`].
#[derive(Debug, Default)]
struct KeptIds {
    /// The kept ids, oldest first until the window is full, then a ring
    /// whose oldest is at `oldest`. It grows by doubling up to
    /// [`ROUTED_ANSWERS_KEPT`] and no further.
    ids: Vec<Uuid>,
    /// The hash of the id at the same position in `ids`, cut to 16 bits:
    /// where its probe starts in the index, which it can be moved in or
    /// rebuilt from without hashing it again.
    hashes: Vec<u16>,
    /// The position in `ids` the next id takes once the window is full: the
    /// oldest id's.
    oldest: usize,
    /// The positions in `ids`, each in the first free slot from its id's
    /// hash on (open addressing, linear probing), [`FREE`] elsewhere: empty,
    /// or a power of two of slots, at least twice as many as ids, so a probe
    /// meets a free slot within a few steps.
    index: Vec<u16>,
    /// Keys the hash: callers choose invocation ids.
    hasher: RandomState,
}

impl RoutedAnswers {
    /// Keeps `invocation_id`, forgetting the oldest past
    /// [`ROUTED_ANSWERS_KEPT`].
    pub fn record(&self, invocation_id: Uuid) {
        lock(&self.kept).record(invocation_id);
    }

    fn contains(&self, invocation_id: &Uuid) -> bool {
        lock(&self.kept).contains(invocation_id)
    }
}

impl KeptIds {
    /// Keeps `id` unless it is kept already, forgetting the oldest past
    /// [`ROUTED_ANSWERS_KEPT`]. A repeated id keeps its place.
    fn record(&mut self, id: Uuid) {
        let hash = self.hash(&id);
        if self.find(&id, hash) {
            return;
        }
        let at = if self.ids.len() < ROUTED_ANSWERS_KEPT {
            self.grow();
            self.ids.push(id);
            self.hashes.push(hash);
            self.ids.len() - 1
        } else {
            let at = self.oldest;
            self.unindex(at);
            self.ids[at] = id;
            self.hashes[at] = hash;
            self.oldest = (at + 1) % ROUTED_ANSWERS_KEPT;
            at
        };
        self.index_at(at);
    }

    fn contains(&self, id: &Uuid) -> bool {
        !self.ids.is_empty() && self.find(id, self.hash(id))
    }

    fn hash(&self, id: &Uuid) -> u16 {
        self.hasher.hash_one(id.as_u128()) as u16
    }

    /// Whether `id`, whose hash is `hash`, is kept.
    fn find(&self, id: &Uuid, hash: u16) -> bool {
        if self.index.is_empty() {
            return false;
        }
        let mask = self.index.len() - 1;
        let mut slot = usize::from(hash) & mask;
        loop {
            match self.index[slot] {
                FREE => return false,
                at if self.ids[usize::from(at)] == *id => return true,
                _ => slot = (slot + 1) & mask,
            }
        }
    }

    /// Indexes position `at` in the first free slot from its id's hash on.
    fn index_at(&mut self, at: usize) {
        let mask = self.index.len() - 1;
        let mut slot = usize::from(self.hashes[at]) & mask;
        while self.index[slot] != FREE {
            slot = (slot + 1) & mask;
        }
        // `at` is below `ROUTED_ANSWERS_KEPT`, which fits in a slot.
        self.index[slot] = at as u16;
    }

    /// Removes position `at` from the index without leaving a tombstone
    /// (backward-shift deletion): each slot after it up to the next free one
    /// moves back into the hole unless the hole lies before its id's hash, so
    /// every probe still meets its id before a free slot.
    fn unindex(&mut self, at: usize) {
        let mask = self.index.len() - 1;
        let mut hole = usize::from(self.hashes[at]) & mask;
        loop {
            match self.index[hole] {
                FREE => return,
                kept if usize::from(kept) == at => break,
                _ => hole = (hole + 1) & mask,
            }
        }
        let mut next = hole;
        loop {
            next = (next + 1) & mask;
            let moved = self.index[next];
            if moved == FREE {
                break;
            }
            // The probe for `moved` runs from its hash to `next`: it moves
            // back when that run crosses the hole.
            let home = usize::from(self.hashes[usize::from(moved)]) & mask;
            if (next.wrapping_sub(home) & mask) >= (next.wrapping_sub(hole) & mask) {
                self.index[hole] = moved;
                hole = next;
            }
        }
        self.index[hole] = FREE;
    }

    /// Makes room for one more id while the window is not full: `ids` and
    /// `hashes` grow by doubling, up to [`ROUTED_ANSWERS_KEPT`] and no
    /// further, and the index doubles, rebuilt, to stay at most half full.
    fn grow(&mut self) {
        let len = self.ids.len();
        if len == self.ids.capacity() {
            let more = len.max(4).min(ROUTED_ANSWERS_KEPT - len);
            self.ids.reserve_exact(more);
            self.hashes.reserve_exact(more);
        }
        if 2 * (len + 1) > self.index.len() {
            self.index = vec![FREE; (2 * self.index.len()).max(8)];
            for at in 0..len {
                self.index_at(at);
            }
        }
    }
}

/// Queue slots an observer keeps once its queue drains; a burst that grew it
/// past this gives the rest back.
const QUEUE_KEPT: usize = 64;

/// Approximate bytes of the events waiting in observers' queues, all queues
/// together, each event counted once however many queues hold it. Past it, an
/// activation that would queue is dropped (`OVERFLOW`) as if its own queue
/// were full: `max_queue_per_observer` bounds one queue, this bounds them all.
/// An event larger than all of it never queues (`TOO_LARGE`).
pub(crate) const QUEUED_BYTES: usize = 64 * 1024 * 1024;

/// How many refused sends [`ObserverRegistry::refused_send`] remembers:
/// enough for each refused activation to find its own, however many other
/// calls were refused meanwhile.
const REFUSED_SENDS_KEPT: usize = 256;

/// What a queued event costs besides its payloads: ids, names, the structures
/// that hold them.
const EVENT_OVERHEAD: usize = 256;

/// The observers' queues together, against [`QUEUED_BYTES`].
pub(crate) struct QueueBudget {
    /// Approximate bytes of the events queued now.
    bytes: AtomicUsize,
    limit: AtomicUsize,
    /// Wakes readers waiting for room ([`ObserverRegistry::wait_for_room`]):
    /// a queue or the budget is down to half, or an observer left `active`.
    pub(crate) room: tokio::sync::Notify,
}

impl Default for QueueBudget {
    fn default() -> Self {
        Self {
            bytes: AtomicUsize::new(0),
            limit: AtomicUsize::new(QUEUED_BYTES),
            room: tokio::sync::Notify::new(),
        }
    }
}

impl QueueBudget {
    /// Charges `event` for one more queue holding it: its cost the first
    /// time, nothing after. `false` when the first charge does not fit.
    fn charge(&self, event: &Completed) -> bool {
        if event.queues.fetch_add(1, Ordering::AcqRel) > 0 {
            return true;
        }
        // A refused charge leaves the cost behind: while the queues are full,
        // each further observer of the event reads it instead of walking the
        // payloads again.
        let cost = match event.charged.load(Ordering::Acquire) {
            0 => event.cost(),
            charged => charged,
        };
        let limit = self.limit.load(Ordering::Relaxed);
        let fits = self
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |bytes| {
                bytes.checked_add(cost).filter(|total| *total <= limit)
            })
            .is_ok();
        // Kept either way: what `too_large` reads.
        event.charged.store(cost, Ordering::Release);
        if !fits {
            event.queues.fetch_sub(1, Ordering::AcqRel);
        }
        fits
    }

    /// `(cost, limit)` when `event` alone is larger than the whole budget:
    /// it never queues, however empty the queues. Known only once a charge
    /// computed its cost.
    fn too_large(&self, event: &Completed) -> Option<(usize, usize)> {
        let cost = event.charged.load(Ordering::Acquire);
        let limit = self.limit.load(Ordering::Relaxed);
        (cost > limit).then_some((cost, limit))
    }

    /// One queue let go of `event`; the last one gives its cost back.
    pub(crate) fn release(&self, event: &Completed) {
        if event.queues.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        let cost = event.charged.load(Ordering::Acquire);
        let half = self.limit.load(Ordering::Relaxed) / 2;
        let before = self.bytes.fetch_sub(cost, Ordering::AcqRel);
        if before > half && before - cost <= half {
            self.room.notify_waiters();
        }
    }

    /// At most half full: a waiting reader may read on.
    fn has_room(&self) -> bool {
        self.bytes.load(Ordering::Acquire) <= self.limit.load(Ordering::Relaxed) / 2
    }
}

/// How an engine-hosted source's event is evaluated.
#[derive(Debug, Default, Clone)]
pub struct Emission {
    /// The depth of what caused the event; `None`: the running invocation's.
    pub depth: Option<u32>,
    /// The trace its activations run in; `None`: a new one each.
    pub traceparent: Option<String>,
}

/// A JSON value's size, roughly: what holding it costs, not its encoding.
fn approx_size(value: &Value) -> usize {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => 16,
        Value::String(s) => 24 + s.len(),
        Value::Array(items) => 24 + items.iter().map(approx_size).sum::<usize>(),
        Value::Object(map) => {
            24 + map
                .iter()
                .map(|(key, value)| 24 + key.len() + approx_size(value))
                .sum::<usize>()
        }
    }
}

/// `delivery` label: the engine matched the call among every active
/// observer.
const ENGINE_DELIVERY: &str = "engine";
/// `delivery` label: the call's host named the observer (its match hook's
/// `matched`, or a report).
const HOST_DELIVERY: &str = "host";
/// `delivery` label: a call through an observer.
const OBSERVER_CALL_DELIVERY: &str = "observer_call";

/// Who matched an event, the `delivery` label of its activations: one byte
/// in the event rather than the label's pointer and length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    /// [`ENGINE_DELIVERY`].
    Engine,
    /// [`HOST_DELIVERY`].
    Host,
}

impl Delivery {
    /// Its `delivery` label.
    fn label(self) -> &'static str {
        match self {
            Self::Engine => ENGINE_DELIVERY,
            Self::Host => HOST_DELIVERY,
        }
    }
}

/// The `function_id` label of a drop that names no observer the engine
/// knows, so no observed function.
const UNKNOWN_FUNCTION: &str = "<unknown>";

/// The stamp the engine adds to the payload of every call a worker makes:
/// the calling connection's id.
const CALLER_STAMP: &str = "_caller_worker_id";

/// `value` as observers receive it: without the engine's top-level
/// `_caller_worker_id` stamp (decision 40). The caller is in `caller`.
fn unstamped(mut value: Value) -> Value {
    if let Value::Object(map) = &mut value {
        map.remove(CALLER_STAMP);
    }
    value
}

/// `{ "worker_name": "iii-engine" }`, the caller of every engine-hosted
/// source's event (what [`InvocationCtx::engine_source`] resolves to): built
/// once and shared by the events, which never change it.
fn engine_source_caller() -> Arc<Value> {
    static CALLER: OnceLock<Arc<Value>> = OnceLock::new();
    CALLER
        .get_or_init(|| {
            let ctx = InvocationCtx::engine_source(0);
            let caller = ctx.caller().and_then(|c| serde_json::to_value(c).ok());
            Arc::new(caller.unwrap_or(Value::Null))
        })
        .clone()
}

/// `text` as observers receive it (see [`unstamped`]): the same text, unless
/// it is an object with a top-level caller stamp, which then goes, the other
/// members kept as they were written. Text that does not spell the stamp, and
/// has no `\u` escape that could, is not read.
fn unstamped_text(text: Arc<str>) -> Arc<str> {
    let may_be_stamped =
        text.starts_with('{') && (text.contains(CALLER_STAMP) || text.contains("\\u"));
    match may_be_stamped
        .then(|| without_caller_stamp(&text))
        .flatten()
    {
        Some(unstamped) => Arc::from(unstamped),
        None => text,
    }
}

/// The JSON object `text` holds, without its top-level caller stamp; `None`
/// when it has none.
fn without_caller_stamp(text: &str) -> Option<String> {
    let Members(members) = serde_json::from_str(text).ok()?;
    if !members.iter().any(|(key, _)| *key == CALLER_STAMP) {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    out.push('{');
    for (key, value) in members.iter().filter(|(key, _)| *key != CALLER_STAMP) {
        if out.len() > 1 {
            out.push(',');
        }
        match key {
            // Read without an escape: written back as it was.
            Cow::Borrowed(key) => {
                out.push('"');
                out.push_str(key);
                out.push('"');
            }
            Cow::Owned(key) => out.push_str(&serde_json::to_string(key).ok()?),
        }
        out.push(':');
        out.push_str(value.get());
    }
    out.push('}');
    Some(out)
}

/// A JSON object's members, in order, each value its raw text.
struct Members<'a>(Vec<(Cow<'a, str>, &'a RawValue)>);

impl<'de> serde::Deserialize<'de> for Members<'de> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Object;

        impl<'de> serde::de::Visitor<'de> for Object {
            type Value = Members<'de>;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an object")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut members = Vec::new();
                while let Some(key) = map.next_key_seed(Key)? {
                    members.push((key, map.next_value()?));
                }
                Ok(Members(members))
            }
        }

        deserializer.deserialize_map(Object)
    }
}

/// An object's key: borrowed from the text when it has no escape.
struct Key;

impl<'de> serde::de::DeserializeSeed<'de> for Key {
    type Value = Cow<'de, str>;

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl<'de> serde::de::Visitor<'de> for Key {
    type Value = Cow<'de, str>;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a key")
    }

    fn visit_borrowed_str<E: serde::de::Error>(self, key: &'de str) -> Result<Self::Value, E> {
        Ok(Cow::Borrowed(key))
    }

    fn visit_str<E: serde::de::Error>(self, key: &str) -> Result<Self::Value, E> {
        Ok(Cow::Owned(key.to_owned()))
    }
}

/// The top-level `fields` of `value` that observers' filters read, without
/// the engine's caller stamp; all of a `value` that is not an object (a
/// filter path into it resolves to nothing either way).
fn project_input(value: &Value, fields: &std::collections::BTreeMap<Box<str>, usize>) -> Value {
    let Value::Object(map) = value else {
        return value.clone();
    };
    Value::Object(
        fields
            .keys()
            .filter(|field| &***field != CALLER_STAMP)
            .filter_map(|field| {
                map.get(&**field)
                    .map(|value| (field.to_string(), value.clone()))
            })
            .collect(),
    )
}

/// What a host that declared `observe.v2` answered a routed call with besides
/// its result (v2 §8.2, decision 55).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostDelivery {
    /// The connection that answered.
    pub host: Uuid,
    /// How the call is delivered.
    pub delivery: RoutedDelivery,
}

/// How a routed call an `observe.v2` host answered is delivered: what its
/// `invocationresult` says with `matched` and `delivered` (decision 55).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutedDelivery {
    /// Neither: the host did not narrow the call's observers (its function
    /// has no match hook). The engine activates every active observer whose
    /// `on` and filters match the call.
    Every,
    /// `matched`: the observers the function's match hook matched, possibly
    /// none. The engine activates those of them that are active observers of
    /// the function and whose `on` and filters match the call.
    Matched(Vec<String>),
    /// `delivered: true`: the function's deliver hook delivered the call. The
    /// engine activates nothing.
    Delivered,
}

impl RoutedDelivery {
    /// What an answer's `matched` and `delivered` say; `delivered` outweighs
    /// `matched`.
    pub fn of(matched: Option<Vec<String>>, delivered: bool) -> Self {
        match (delivered, matched) {
            (true, _) => Self::Delivered,
            (false, None) => Self::Every,
            (false, Some(ids)) => Self::Matched(ids),
        }
    }

    /// The observers the host's match hook named: none unless `Matched`.
    pub fn named(&self) -> &[String] {
        match self {
            Self::Matched(ids) => ids,
            Self::Every | Self::Delivered => &[],
        }
    }
}

/// Which of a completion's active observers it activates: those whose `on`
/// and filters match it, among all of them or among the ones its host's
/// match hook named.
enum Matching {
    /// Among every active observer: the engine matched the call by itself.
    Filters,
    /// Among the ones the call's host named (sorted, each once): its
    /// function's match hook matched them.
    Matched(Vec<String>),
}

impl Matching {
    /// The `delivery` label of the activations it starts: who chose the
    /// observers it evaluates.
    fn delivery(&self) -> Delivery {
        match self {
            Self::Filters => Delivery::Engine,
            Self::Matched(_) => Delivery::Host,
        }
    }

    /// Whether the completion may activate `observer`: what a drop of the
    /// whole completion is charged to.
    fn concerns(&self, observer: &Observer) -> bool {
        match self {
            Self::Filters => true,
            Self::Matched(ids) => is_listed(ids, &observer.id),
        }
    }
}

/// Whether the sorted `ids` hold `id`.
fn is_listed(ids: &[String], id: &str) -> bool {
    ids.binary_search_by(|listed| listed.as_str().cmp(id))
        .is_ok()
}

/// A completed observed invocation (or a reported one), as its activations
/// see it. Payloads are shared: each activation clones what it ships. Each
/// is a [`Payload`]: a value, or JSON text that a worker's activation sends
/// as is and that is decoded, once, for a target that takes a value. A
/// report's payloads stay the text its host sent.
pub(crate) struct Completed {
    /// Who matched it, as the `delivery` label of
    /// `iii.observer.activations.total`: [`ENGINE_DELIVERY`] or
    /// [`HOST_DELIVERY`].
    delivery: Delivery,
    /// The observed function, `(namespace, function_id)`: the key its active
    /// set holds, shared rather than copied.
    function: Arc<FnKey>,
    invocation_id: InvocationId,
    success: bool,
    /// Whether its output is its input (a report's `output_is_input`): held
    /// once, as `input`, with `output` empty.
    output_is_input: bool,
    input: Option<Payload>,
    output: Option<Payload>,
    error: Option<Payload>,
    caller: Option<Arc<Value>>,
    metadata: Option<Payload>,
    started_at_ms: u64,
    duration_ms: f64,
    depth: u32,
    /// The call's trace context, boxed: no spare capacity to hold.
    traceparent: Option<Box<str>>,
    baggage: Option<Box<str>>,
    /// Queues holding it now (see [`QueueBudget`]).
    queues: AtomicUsize,
    /// What it was charged when the first of them took it (or would have
    /// been, when that did not fit).
    charged: AtomicUsize,
    /// Its payloads held as values, as JSON text, for the activations a
    /// worker connection receives (a payload held as text is sent as it
    /// is): allocated by the first of them that encodes one, so an event
    /// that only in-process targets read holds none. One pointer: two first
    /// activations racing may both allocate it, and the loser's is dropped.
    encoded: OnceBox<EncodedParts>,
}

/// A completion's `invocation_id`: the engine's own id kept as a [`Uuid`]
/// until an activation writes it, or the id a report gave, as it gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InvocationId {
    Uuid(Uuid),
    Reported(String),
}

impl InvocationId {
    /// The id a report gave: kept as a [`Uuid`] when it is one written the
    /// way the engine writes it (hyphenated, lowercase), which gives back the
    /// same text, and as given otherwise. `parsed` is the id as a [`Uuid`],
    /// when it parses as one.
    fn reported(id: String, parsed: Option<Uuid>) -> Self {
        match parsed {
            Some(uuid) if *uuid.hyphenated().encode_lower(&mut Uuid::encode_buffer()) == *id => {
                Self::Uuid(uuid)
            }
            _ => Self::Reported(id),
        }
    }

    /// Its length as text.
    fn len(&self) -> usize {
        match self {
            Self::Uuid(_) => uuid::fmt::Hyphenated::LENGTH,
            Self::Reported(id) => id.len(),
        }
    }
}

impl std::fmt::Display for InvocationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Uuid(uuid) => std::fmt::Display::fmt(&uuid.hyphenated(), f),
            Self::Reported(id) => f.write_str(id),
        }
    }
}

impl serde::Serialize for InvocationId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Uuid(uuid) => {
                serializer.serialize_str(uuid.hyphenated().encode_lower(&mut Uuid::encode_buffer()))
            }
            Self::Reported(id) => serializer.serialize_str(id),
        }
    }
}

/// A completion's payloads as JSON text, each encoded the first time an
/// activation whose target a worker connection hosts needs it, then shared
/// by all of them: one encoding per event, not one per observer.
#[derive(Default)]
pub(crate) struct EncodedParts {
    input: OnceLock<Option<Arc<str>>>,
    output: OnceLock<Option<Arc<str>>>,
    error: OnceLock<Option<Arc<str>>>,
    caller: OnceLock<Option<Arc<str>>>,
    metadata: OnceLock<Option<Arc<str>>>,
}

/// The [`approx_size`] above which a call keeps a payload an observer needs
/// whole as its JSON text rather than as a copy: a copy allocates every node
/// on the caller's path, the text one buffer, which a worker's activation
/// sends as is. Below it a copy costs less than encoding and decoding.
const TEXT_ABOVE: usize = 2 * 1024;

/// One payload of an event (its input, output, error or metadata), held
/// once: as a value, or as its JSON text alone, when the call kept it so (see
/// [`TEXT_ABOVE`]) or a report carried it. A value is built from the text only for what reads one: the
/// observers' filters, for their evaluation alone (see [`Payload::filtered`]),
/// and the activations whose target takes a value, which share the one the
/// first of them decodes (see [`Payload::value`]).
enum Payload {
    Value(Arc<Value>),
    /// Boxed, so that a payload is a pointer and a tag: most events hold
    /// small payloads, as values.
    Text(Box<PayloadText>),
}

/// A payload held as its JSON text.
struct PayloadText {
    text: Arc<str>,
    /// Decoded from `text` when the first activation that takes a value
    /// starts; `None` inside when the text does not decode.
    value: OnceLock<Option<Arc<Value>>>,
}

impl Payload {
    /// `text`, a JSON value's text, held as it is.
    fn text(text: Arc<str>) -> Self {
        Payload::Text(Box::new(PayloadText {
            text,
            value: OnceLock::new(),
        }))
    }

    /// `value` as observers see it (see [`unstamped`]): copied, or encoded
    /// when larger than [`TEXT_ABOVE`].
    fn of(value: &Value) -> Self {
        if approx_size(value) > TEXT_ABOVE {
            Payload::text(encode_unstamped(value))
        } else {
            Payload::Value(Arc::new(unstamped(value.clone())))
        }
    }

    /// Its value, for an activation whose target takes one: the value it
    /// holds, or the one the first such activation decodes from its text,
    /// kept from then on for the event's other activations.
    fn value(&self) -> Option<&Arc<Value>> {
        match self {
            Payload::Value(value) => Some(value),
            Payload::Text(text) => text
                .value
                .get_or_init(|| serde_json::from_str(&text.text).ok().map(Arc::new))
                .as_ref(),
        }
    }

    /// Its value, for the observers' filters: the value it holds, or, when
    /// it holds text that a filter reads (`read`), the value an activation
    /// already decoded, or else one decoded for the evaluation alone, which
    /// the event does not keep. Text no filter reads is not decoded.
    fn filtered(&self, read: bool) -> Option<Cow<'_, Value>> {
        match self {
            Payload::Value(value) => Some(Cow::Borrowed(value)),
            Payload::Text(_) if !read => None,
            Payload::Text(text) => match text.value.get() {
                Some(decoded) => decoded.as_deref().map(Cow::Borrowed),
                None => serde_json::from_str(&text.text).ok().map(Cow::Owned),
            },
        }
    }

    /// Roughly what holding it costs (see [`QUEUED_BYTES`] and
    /// [`EVALUATOR_BYTES`]): its value's [`approx_size`], or its text's
    /// length, plus the value decoded from it once there is one.
    fn size(&self) -> usize {
        match self {
            Payload::Value(value) => approx_size(value),
            Payload::Text(text) => {
                24 + text.text.len()
                    + text
                        .value
                        .get()
                        .and_then(Option::as_deref)
                        .map_or(0, approx_size)
            }
        }
    }
}

/// A payload as observers see it (see [`unstamped`]), as JSON text.
fn encode_unstamped(value: &Value) -> Arc<str> {
    Arc::from(serde_json::to_string(&Unstamped(value)).unwrap_or_else(|_| "null".to_string()))
}

/// A value as observers see it, serialized: without the engine's top-level
/// caller stamp, and without copying the value to drop it.
struct Unstamped<'a>(&'a Value);

impl serde::Serialize for Unstamped<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self.0 {
            Value::Object(map) if map.contains_key(CALLER_STAMP) => {
                let mut out = serializer.serialize_map(Some(map.len() - 1))?;
                for (key, value) in map {
                    if key != CALLER_STAMP {
                        out.serialize_entry(key, value)?;
                    }
                }
                out.end()
            }
            value => value.serialize(serializer),
        }
    }
}

/// `value`'s JSON text, encoded into `cell` the first time.
fn encode_once(cell: &OnceLock<Option<Arc<str>>>, value: Option<&Value>) -> Option<Arc<str>> {
    cell.get_or_init(|| {
        value
            .and_then(|value| serde_json::to_string(value).ok())
            .map(Arc::from)
    })
    .clone()
}

impl Completed {
    /// Its payloads' text, allocated the first time an activation encodes
    /// one.
    fn encoded(&self) -> &EncodedParts {
        self.encoded.get_or_init(Box::default)
    }

    /// `payload`'s JSON text: the text it holds, or its value's, encoded
    /// into its `cell` of the event's [`EncodedParts`] the first time.
    fn encode_payload(
        &self,
        payload: Option<&Payload>,
        cell: fn(&EncodedParts) -> &OnceLock<Option<Arc<str>>>,
    ) -> Option<Arc<str>> {
        match payload? {
            Payload::Text(text) => Some(text.text.clone()),
            Payload::Value(value) => encode_once(cell(self.encoded()), Some(value)),
        }
    }

    fn encoded_input(&self) -> Option<Arc<str>> {
        self.encode_payload(self.input.as_ref(), |parts| &parts.input)
    }

    fn encoded_output(&self) -> Option<Arc<str>> {
        // A report's `output_is_input`: the input's text.
        if self.output_is_input {
            return self.encoded_input();
        }
        // An event source's input and output are the same event: encoded
        // once.
        if let (Some(Payload::Value(input)), Some(Payload::Value(output))) =
            (&self.input, &self.output)
            && Arc::ptr_eq(input, output)
        {
            return self.encoded_input();
        }
        self.encode_payload(self.output.as_ref(), |parts| &parts.output)
    }

    fn encoded_error(&self) -> Option<Arc<str>> {
        self.encode_payload(self.error.as_ref(), |parts| &parts.error)
    }

    fn encoded_caller(&self) -> Option<Arc<str>> {
        let caller = self.caller.as_deref()?;
        encode_once(&self.encoded().caller, Some(caller))
    }

    fn encoded_metadata(&self) -> Option<Arc<str>> {
        self.encode_payload(self.metadata.as_ref(), |parts| &parts.metadata)
    }

    /// Roughly what holding the event costs (see [`QUEUED_BYTES`]): a
    /// payload held as text counts its length (see [`Payload::size`]), once
    /// when it is the output too (a report's `output_is_input`).
    fn cost(&self) -> usize {
        // An event source's input and output are one value (CONTRACT D3):
        // held once, counted once.
        let output = match (&self.input, &self.output) {
            (Some(Payload::Value(input)), Some(Payload::Value(output)))
                if Arc::ptr_eq(input, output) =>
            {
                &None
            }
            _ => &self.output,
        };
        let payloads = [&self.input, output, &self.error, &self.metadata]
            .into_iter()
            .flatten()
            .map(Payload::size)
            .sum::<usize>()
            + self.caller.as_deref().map_or(0, approx_size);
        let text = [&self.traceparent, &self.baggage]
            .into_iter()
            .filter_map(|value| value.as_deref())
            .map(str::len)
            .sum::<usize>();
        EVENT_OVERHEAD
            + self.function.0.len()
            + self.function.1.len()
            + self.invocation_id.len()
            + text
            + payloads
    }
}

/// One routed completion waiting for the evaluator.
pub(crate) struct Evaluation {
    /// Approximate size of the payloads it holds, against the evaluator's
    /// byte budget.
    bytes: usize,
    set: Arc<ActiveSet>,
    matching: Matching,
    completed: Completed,
    /// Turned into the document's `error` only when an observer needs it.
    error: Option<ErrorBody>,
    /// Resolved into the document's `caller` only when an observer needs it.
    /// `None` for an engine-hosted source's event, whose caller is the engine
    /// (see [`engine_source_caller`]).
    ctx: Option<Arc<InvocationCtx>>,
}

impl Evaluation {
    /// The active observers it may activate.
    fn concerned(&self) -> impl Iterator<Item = &Arc<Observer>> {
        self.set
            .observers
            .iter()
            .filter(|observer| self.matching.concerns(observer))
    }
}

/// An observer's in-flight slot. Released on drop unless handed on, so a
/// panic while delivering cannot leak the slot.
struct Slot {
    registry: Arc<ObserverRegistry>,
    observer: Arc<Observer>,
    held: bool,
}

impl Drop for Slot {
    fn drop(&mut self) {
        if self.held {
            self.registry.release_slot(&self.observer);
        }
    }
}

impl ObserverRegistry {
    /// The active observers of `(namespace, function_id)`, when it has any:
    /// what [`Self::route`] answers, without the caller check. One atomic
    /// load while nothing is observed or hosted privately by a connection;
    /// otherwise the view's load and one hash lookup more, and the
    /// function's set cloned under its read lock when the view holds the
    /// function.
    #[inline]
    pub fn watch(&self, namespace: &str, function_id: &str) -> Option<Arc<ActiveSet>> {
        if self.routed_live.load(Ordering::Acquire) == 0 {
            return None;
        }
        let key = &(namespace, function_id) as &dyn crate::function::FunctionKey;
        self.routed
            .load(key)
            .get(key)
            .and_then(|routed| routed.active.load_full())
    }

    /// Whether `(namespace, function_id)` has an active observer: what an
    /// event source checks before building an event.
    #[inline]
    pub fn is_observed(&self, namespace: &str, function_id: &str) -> bool {
        let key = &(namespace, function_id) as &dyn crate::function::FunctionKey;
        self.routed_live.load(Ordering::Acquire) != 0
            && self
                .routed
                .load(key)
                .get(key)
                .is_some_and(|routed| routed.active.is_some())
    }

    /// One call of an engine-hosted source (see [`super::events`]): evaluated
    /// like a successful invocation of `(namespace, function_id)` whose input
    /// and output are both the event (the source's handler returns it), with
    /// `{ worker_name: "iii-engine" }` as the caller and the depth of the
    /// invocation that caused it. `build` runs only when an active observer
    /// needs the event and the evaluator has room; unobserved, this costs what
    /// [`Self::watch`] costs.
    pub fn emit(
        self: &Arc<Self>,
        namespace: &str,
        function_id: &str,
        build: impl FnOnce() -> Option<Value>,
    ) {
        self.emit_with(namespace, function_id, Emission::default(), build);
    }

    /// [`Self::emit`] with the event's depth and trace given (an event
    /// produced away from the invocation that caused it: a log record
    /// ingested later, a coalesced burst).
    pub fn emit_with(
        self: &Arc<Self>,
        namespace: &str,
        function_id: &str,
        emission: Emission,
        build: impl FnOnce() -> Option<Value>,
    ) {
        if let Some(evaluation) = self.emission(namespace, function_id, emission, build) {
            self.enqueue(evaluation);
        }
    }

    /// The evaluation of one call of the engine-hosted source `(namespace,
    /// function_id)` (see [`Self::emit_with`]), when it has active observers
    /// and the evaluator has room.
    pub(super) fn emission(
        &self,
        namespace: &str,
        function_id: &str,
        emission: Emission,
        build: impl FnOnce() -> Option<Value>,
    ) -> Option<Evaluation> {
        let set = self.watch(namespace, function_id)?;
        if !self.evaluator_has_room(&set) {
            for observer in &set.observers {
                drop_event(observer, observe_codes::OVERFLOW);
            }
            return None;
        }
        let event = if set.needs.input || set.needs.output {
            self.events_built.fetch_add(1, Ordering::Relaxed);
            Some(Arc::new(build()?))
        } else {
            None
        };
        let depth = emission.depth.unwrap_or_else(InvocationCtx::current_depth);
        Some(Evaluation {
            bytes: event.as_deref().map_or(0, approx_size),
            completed: Completed {
                delivery: Delivery::Engine,
                function: set.function.clone(),
                invocation_id: InvocationId::Uuid(super::new_uuid()),
                success: true,
                output_is_input: false,
                input: event
                    .clone()
                    .filter(|_| set.needs.input)
                    .map(Payload::Value),
                output: event.filter(|_| set.needs.output).map(Payload::Value),
                error: None,
                caller: None,
                metadata: None,
                started_at_ms: now_ms(),
                duration_ms: 0.0,
                depth,
                traceparent: emission.traceparent.map(String::into_boxed_str),
                baggage: None,
                queues: AtomicUsize::new(0),
                charged: AtomicUsize::new(0),
                encoded: OnceBox::new(),
            },
            set,
            matching: Matching::Filters,
            error: None,
            ctx: None,
        })
    }

    /// Starts tracking an observed invocation: keeps the input, and shares
    /// the call's metadata (CONTRACT D3: one more reference, never a copy),
    /// only when an active observer needs them (its filters read them, or it
    /// captures or delivers them), and only when the evaluator has room
    /// (otherwise the call counts an `OVERFLOW` drop when it completes,
    /// without having copied anything). Whether only the host's answer says
    /// how the call is delivered is decided when the call begins, from the
    /// host it goes to.
    ///
    /// Of the input it keeps the fields the observers' filters read when that
    /// is all they read of it, and otherwise all of it: a copy, or, past
    /// [`TEXT_ABOVE`], its JSON text (one buffer rather than a copy of every
    /// node on the caller's path), which the event holds alone (see
    /// [`Payload`]) and a worker's activation sends as is.
    ///
    /// Observers see the payload as the caller sent it: a call a worker makes
    /// carries the engine's `_caller_worker_id` stamp, which what is kept
    /// leaves out (the caller is `caller.worker_id`).
    pub fn begin(
        &self,
        set: Arc<ActiveSet>,
        body: &Value,
        metadata: &Option<Arc<Value>>,
        traceparent: &Option<String>,
        baggage: &Option<String>,
    ) -> ObservedCall {
        let admitted = self.evaluator_has_room(&set);
        let host_delivers = self.host_delivers(&set.function);
        let needs = set.needs;
        let input = if admitted && needs.input {
            self.payload_copies.fetch_add(1, Ordering::Relaxed);
            Some(match set.input.projection() {
                // Observers that only filter on the input get the fields
                // their filters read: the rest is not copied.
                Some(fields) => Payload::Value(Arc::new(project_input(body, fields))),
                // One that needs all of it gets a copy, or its JSON text
                // when it is large (see [`TEXT_ABOVE`]).
                None => Payload::of(body),
            })
        } else {
            None
        };
        let metadata = if admitted && needs.metadata {
            metadata.clone()
        } else {
            None
        };
        ObservedCall {
            inner: Some(Tracked {
                set,
                host_delivers,
                needs,
                admitted,
                started: Instant::now(),
                started_at_ms: now_ms(),
                input,
                metadata,
                traceparent: traceparent.clone(),
                baggage: baggage.clone(),
            }),
        }
    }

    /// `(running, queued)` activations of observer `id`, for introspection.
    pub fn in_flight(&self, id: &str) -> Option<(u32, usize)> {
        self.get(id).map(|observer| {
            let flow = observer.flow.write();
            (flow.running, flow.queue.len())
        })
    }

    /// How many times the routed hook copied an invocation's input. Zero as
    /// long as nothing that needs the input is observed.
    pub fn payload_copies(&self) -> u64 {
        self.payload_copies.load(Ordering::Relaxed)
    }

    /// Events an event source built because an observer needed them. Zero
    /// as long as nothing observes the event sources.
    pub fn events_built(&self) -> u64 {
        self.events_built.load(Ordering::Relaxed)
    }

    /// Observed routed invocations the evaluators handled so far: evaluated
    /// against every active observer, or against the ones their host's match
    /// hook named.
    pub fn evaluated(&self) -> u64 {
        self.evaluated.load(Ordering::Acquire)
    }

    /// Times a reporting connection's reader waited for room in a queue its
    /// report overflowed (see [`Self::wait_for_room`]).
    pub fn report_waits(&self) -> u64 {
        self.report_waits.load(Ordering::Relaxed)
    }

    /// Hands the completed invocation to the evaluator: the caller's reply
    /// goes out without waiting for filters or activations. Keeps the output
    /// only when an active observer needs it.
    ///
    /// `delivered` is what an `observe.v2` host answered besides the result
    /// (decision 55): nothing, and the engine evaluates every active
    /// observer; the observers its match hook matched, and the engine
    /// evaluates those of them that are active observers of the function; or
    /// that its deliver hook delivered the call, and nothing is activated.
    /// The answer's result is the observers' `output`, and its `error` their
    /// `error` (decisions 36, 39). Only the function's host answers for its
    /// observers: an answer from any other `observe.v2` connection (a caller
    /// answering its own call by its id, say) activates nothing, whatever it
    /// carries, and counts `REPORTER_NOT_OWNER` for each observer it names
    /// in `matched`, or for each active observer when it names none (see
    /// [`Self::refuse_answer`]). Without `delivered`, a call whose host
    /// declared `observe.v2` ended without its answer: each active observer
    /// counts a `CANCELLED` drop, since only the host's match and deliver
    /// hooks know which of them it is for. The engine evaluates the
    /// observers of every other call against all of them.
    ///
    /// When the evaluator has no room for the call, nothing is evaluated:
    /// each observer the call could activate counts an `OVERFLOW` drop,
    /// every active observer, or the ones the host's match hook named, their
    /// filters unread (the input was not kept, and an overloaded engine
    /// evaluates no filter on the caller's path).
    #[allow(clippy::too_many_arguments)]
    pub fn complete(
        self: &Arc<Self>,
        mut call: ObservedCall,
        namespace: &str,
        function_id: &str,
        invocation_id: Uuid,
        result: &Result<Result<Option<Value>, ErrorBody>, RecvError>,
        delivered: Option<HostDelivery>,
        ctx: &Arc<InvocationCtx>,
    ) {
        let Some(call) = call.inner.take() else {
            return;
        };
        let matching = match delivered {
            None if call.host_delivers => {
                for observer in &call.set.observers {
                    drop_event(observer, CANCELLED);
                }
                return;
            }
            None => Matching::Filters,
            Some(HostDelivery { host, delivery }) => {
                if !self.hosts(host, namespace, function_id) {
                    self.refuse_answer(&call.set, namespace, function_id, &delivery);
                    return;
                }
                match delivery {
                    RoutedDelivery::Delivered => return,
                    RoutedDelivery::Every => Matching::Filters,
                    RoutedDelivery::Matched(ids) => {
                        if ids.is_empty() {
                            return;
                        }
                        let mut ids = ids;
                        ids.sort_unstable();
                        ids.dedup();
                        Matching::Matched(ids)
                    }
                }
            }
        };
        if !call.admitted || !self.evaluator_has_room(&call.set) {
            for observer in call.set.observers.iter().filter(|o| matching.concerns(o)) {
                drop_event(observer, observe_codes::OVERFLOW);
            }
            return;
        }
        let needs = call.needs;
        // The output as the input: a copy, or its JSON text when it is large.
        let (success, output, error) = match result {
            Ok(Ok(output)) => (
                true,
                if needs.output {
                    output.as_ref().map(Payload::of)
                } else {
                    None
                },
                None,
            ),
            Ok(Err(error)) => (false, None, needs.error.then(|| error.clone())),
            Err(err) => (
                false,
                None,
                needs
                    .error
                    .then(|| ErrorBody::new("invocation_error", err.to_string())),
            ),
        };
        // Everything the evaluator queue holds for the call: its input and
        // output (a payload kept as text counts its length, see
        // [`Payload::size`]) and its metadata. The metadata is the caller's,
        // shared rather than copied, but the event keeps it alive past the
        // call, so it is charged as held (CONTRACT D3).
        let bytes = [&call.input, &output]
            .into_iter()
            .flatten()
            .map(Payload::size)
            .sum::<usize>()
            + call.metadata.as_deref().map_or(0, approx_size);
        // The set the call began with is the function's: the event shares
        // its key.
        debug_assert!(call.set.function.0 == namespace && call.set.function.1 == function_id);
        let evaluation = Evaluation {
            bytes,
            completed: Completed {
                delivery: matching.delivery(),
                function: call.set.function.clone(),
                invocation_id: InvocationId::Uuid(invocation_id),
                success,
                output_is_input: false,
                input: call.input,
                output,
                error: None,
                caller: None,
                metadata: call.metadata.map(Payload::Value),
                started_at_ms: call.started_at_ms,
                duration_ms: call.started.elapsed().as_secs_f64() * 1000.0,
                depth: ctx.observe_depth,
                traceparent: call.traceparent.map(String::into_boxed_str),
                baggage: call.baggage.map(String::into_boxed_str),
                queues: AtomicUsize::new(0),
                charged: AtomicUsize::new(0),
                encoded: OnceBox::new(),
            },
            set: call.set,
            matching,
            error,
            ctx: Some(ctx.clone()),
        };
        self.enqueue(evaluation);
    }

    /// An `observe.v2` answer to a routed call of `(namespace, function_id)`
    /// from a connection that does not host it: it activates nothing, and
    /// each observer it names in `matched` counts `REPORTER_NOT_OWNER` (an
    /// empty `matched` counts one against the function); without `matched`,
    /// or with `delivered`, it names none, and each of `set`'s active
    /// observers does, since the answer would have decided for all of them.
    fn refuse_answer(
        &self,
        set: &ActiveSet,
        namespace: &str,
        function_id: &str,
        delivery: &RoutedDelivery,
    ) {
        match delivery {
            RoutedDelivery::Matched(ids) => {
                let key = (namespace.to_string(), function_id.to_string());
                self.drop_matched(&key, ids, observe_codes::REPORTER_NOT_OWNER);
            }
            RoutedDelivery::Every | RoutedDelivery::Delivered => {
                for observer in &set.observers {
                    drop_event(observer, observe_codes::REPORTER_NOT_OWNER);
                }
            }
        }
    }

    /// Whether `connection` hosts `(namespace, function_id)` now.
    fn hosts(&self, connection: Uuid, namespace: &str, function_id: &str) -> bool {
        self.deps.get().is_some_and(|deps| {
            deps.function_owners
                .get(&(namespace, function_id) as &dyn crate::function::FunctionKey)
                .is_some_and(|owner| owner.0 == connection)
        })
    }

    /// The observers a host's match hook named for a call of `(namespace,
    /// function_id)` that had no active observer when it began: the engine
    /// kept nothing to build their events from, so each is counted as
    /// `INACTIVE`.
    pub(crate) fn not_held(&self, namespace: &str, function_id: &str, observed: &[String]) {
        for id in observed {
            self.drop_not_held(namespace, function_id, id);
        }
    }

    /// One id a host named that the call cannot activate: an observer of the
    /// function that was not active when the call began, or no observer of
    /// it (gone since, or another function's). `INACTIVE`, as for a report
    /// item naming one.
    fn drop_not_held(&self, namespace: &str, function_id: &str, id: &str) {
        match self
            .get(id)
            .filter(|o| o.function.0 == namespace && o.function.1 == function_id)
        {
            Some(observer) => drop_event(&observer, observe_codes::INACTIVE),
            None => count_function_drop(function_id, observe_codes::INACTIVE, 1),
        }
    }

    /// Whether `set`'s evaluator can take one more completion: under both
    /// its count ([`EVALUATOR_QUEUE`]) and the evaluators' byte budget
    /// ([`EVALUATOR_BYTES`], approximate payload sizes).
    fn evaluator_has_room(&self, set: &ActiveSet) -> bool {
        self.evaluators
            .get()
            .is_none_or(|evaluators| evaluator_of(evaluators, set).capacity() > 0)
            && self.queued_bytes.load(Ordering::Acquire)
                < self.evaluator_bytes.load(Ordering::Relaxed)
    }

    /// Hands `evaluation` to its function's evaluator task; inline when there
    /// is none. Past the queue's bounds the completion is dropped as
    /// `OVERFLOW` for each active observer of the function; larger than the
    /// whole byte budget, as `TOO_LARGE`.
    fn enqueue(self: &Arc<Self>, evaluation: Evaluation) {
        if let Some(evaluation) = self.try_enqueue(evaluation) {
            self.evaluate(evaluation);
        }
    }

    /// [`Self::enqueue`], except that an evaluation no evaluator task can
    /// take is returned, for the caller to evaluate (see
    /// [`ObserverRegistry::evaluate`]) once it holds no lock an activation
    /// may take. A function's evaluations reach its evaluator in the order
    /// they are handed over.
    pub(super) fn try_enqueue(self: &Arc<Self>, evaluation: Evaluation) -> Option<Evaluation> {
        let Some(evaluators) = self.evaluators() else {
            return Some(evaluation);
        };
        let evaluator = evaluator_of(evaluators, &evaluation.set);
        let bytes = evaluation.bytes;
        let budget = self.evaluator_bytes.load(Ordering::Relaxed);
        if bytes > budget {
            for observer in evaluation.concerned() {
                drop_too_large(observer, bytes, budget, TooLarge::Held);
            }
            return None;
        }
        // Reserved, not checked then added: concurrent completions cannot
        // overshoot the budget together.
        let reserved = self
            .queued_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |queued| {
                queued.checked_add(bytes).filter(|total| *total <= budget)
            })
            .is_ok();
        if !reserved {
            for observer in evaluation.concerned() {
                drop_event(observer, observe_codes::OVERFLOW);
            }
            return None;
        }
        match evaluator.try_send(evaluation) {
            Ok(()) => None,
            Err(mpsc::error::TrySendError::Full(evaluation)) => {
                self.queued_bytes.fetch_sub(bytes, Ordering::AcqRel);
                for observer in evaluation.concerned() {
                    drop_event(observer, observe_codes::OVERFLOW);
                }
                None
            }
            Err(mpsc::error::TrySendError::Closed(evaluation)) => {
                self.queued_bytes.fetch_sub(bytes, Ordering::AcqRel);
                Some(evaluation)
            }
        }
    }

    /// The evaluator tasks' queues, started on first use: one per runtime
    /// worker thread, up to [`MAX_EVALUATORS`].
    fn evaluators(self: &Arc<Self>) -> Option<&[mpsc::Sender<Evaluation>]> {
        if let Some(evaluators) = self.evaluators.get() {
            return Some(evaluators);
        }
        let runtime = self.runtime()?;
        let count = runtime.metrics().num_workers().clamp(1, MAX_EVALUATORS);
        let capacity = self.evaluator_capacity.load(Ordering::Relaxed).max(1);
        let (senders, receivers): (Vec<_>, Vec<_>) = (0..count)
            .map(|_| mpsc::channel::<Evaluation>(capacity))
            .unzip();
        if self.evaluators.set(senders).is_ok() {
            for mut rx in receivers {
                let registry = Arc::downgrade(self);
                runtime.spawn(async move {
                    while let Some(evaluation) = rx.recv().await {
                        let Some(registry) = registry.upgrade() else {
                            return;
                        };
                        registry
                            .queued_bytes
                            .fetch_sub(evaluation.bytes, Ordering::AcqRel);
                        registry.evaluate(evaluation);
                    }
                });
            }
        }
        self.evaluators.get().map(Vec::as_slice)
    }

    /// Activates the observers one completed invocation matched: those whose
    /// `on` and filters match it, among every active observer (looked up in
    /// the function's filter index, or scanned when it has none) or among the
    /// ones its host's match hook named, in creation order.
    pub(super) fn evaluate(self: &Arc<Self>, evaluation: Evaluation) {
        let Evaluation {
            set,
            matching,
            mut completed,
            error,
            ctx,
            ..
        } = evaluation;
        let needs = set.needs;
        completed.error = error
            .and_then(|e| serde_json::to_value(e).ok())
            .map(|e| Payload::Value(Arc::new(e)));
        if needs.caller {
            completed.caller = match &ctx {
                Some(ctx) => ctx
                    .caller()
                    .and_then(|c| serde_json::to_value(c).ok())
                    .map(Arc::new),
                None => Some(engine_source_caller()),
            };
        }
        let completed = Arc::new(completed);
        // A payload the call kept as text is decoded for the filters only
        // when one of them reads it, and that value is let go of once they
        // are evaluated: a queued event holds the text alone (see
        // [`Payload`]).
        let filtered = set.filtered();
        let input = completed
            .input
            .as_ref()
            .and_then(|input| input.filtered(filtered.input));
        let output = completed
            .output
            .as_ref()
            .and_then(|output| output.filtered(filtered.output));
        // A call's error and metadata are held as values.
        let error = completed.error.as_ref().and_then(|e| e.filtered(true));
        let metadata = completed.metadata.as_ref().and_then(|m| m.filtered(true));
        let doc = Doc {
            input: input.as_deref(),
            output: output.as_deref(),
            error: error.as_deref(),
            caller: completed.caller.as_deref(),
            metadata: metadata.as_deref(),
        };
        // Read once for every observer the call may activate.
        let max_depth = self.config.load().max_observe_depth;
        match matching {
            Matching::Filters => match &set.filters {
                Some(filters) => {
                    for (_, observer) in filters.matches(&doc) {
                        if observer.capture.fires(completed.success) {
                            self.activate_within(observer, &completed, max_depth);
                        }
                    }
                }
                // No filter the index can serve: a scan is what it would do.
                None => {
                    for leaf in set.observers.leaves() {
                        for observer in leaf {
                            if observer.capture.fires(completed.success)
                                && observer.filter.matches(&doc)
                            {
                                self.activate_within(observer, &completed, max_depth);
                            }
                        }
                    }
                }
            },
            Matching::Matched(ids) => {
                let mut held = Vec::with_capacity(ids.len());
                for id in &ids {
                    match set.held(self, id) {
                        Some(observer) => held.push(observer),
                        None => {
                            self.drop_not_held(&completed.function.0, &completed.function.1, id)
                        }
                    }
                }
                held.sort_by_key(|observer| observer.seq());
                for observer in &held {
                    if observer.capture.fires(completed.success) && observer.filter.matches(&doc) {
                        self.activate_within(observer, &completed, max_depth);
                    }
                }
            }
        }
        self.evaluated.fetch_add(1, Ordering::AcqRel);
    }

    /// Policy for one matched observer (protocol.md §3.4): state, loop guard,
    /// expiry, `max_fires`, then an in-flight slot or the bounded queue.
    /// Returns whether the event was dropped because the queue was full.
    fn activate(self: &Arc<Self>, observer: &Arc<Observer>, completed: &Arc<Completed>) -> bool {
        let max_depth = self.config.load().max_observe_depth;
        self.activate_within(observer, completed, max_depth)
    }

    /// [`Self::activate`] with the loop guard's `max_depth` given: what an
    /// evaluation reads once for all the observers it activates.
    fn activate_within(
        self: &Arc<Self>,
        observer: &Arc<Observer>,
        completed: &Arc<Completed>,
        max_depth: u32,
    ) -> bool {
        if !observer.is_active() {
            drop_event(observer, observe_codes::INACTIVE);
            return false;
        }
        if completed.depth.saturating_add(1) > max_depth {
            drop_event(observer, observe_codes::LOOP_GUARD);
            warn_loop_guard(observer, completed.depth, max_depth);
            return false;
        }
        if observer.is_expired() {
            self.dispose(
                observer,
                observe_codes::EXPIRED,
                "the observer's expires_in_ms elapsed",
            );
            drop_event(observer, observe_codes::INACTIVE);
            return false;
        }
        // With `max_fires`, the fire is taken first, so a spent observer drops
        // as `MAX_FIRES`; without it, the fire is counted under the flow lock
        // once the activation has room, and a full queue's drop counts nothing
        // else.
        let taken = match observer.policy.max_fires {
            Some(_) => match take_fire(observer) {
                Some(fired) => Some(fired),
                None => return false,
            },
            None => None,
        };
        let give_back = || {
            if taken.is_some() {
                observer.fires.fetch_sub(1, Ordering::AcqRel);
            }
        };
        let (run_now, fired) = {
            let mut flow = observer.flow.write();
            // Disposal sets the state and then drains this queue under the
            // lock: judging again here keeps an activation that passed the
            // check above from running after it. A `MAX_FIRES` disposal
            // keeps the queue, and this fire is within `max_fires`: it runs.
            if !observer.runs_counted_fires() {
                drop(flow);
                give_back();
                drop_event(observer, observe_codes::INACTIVE);
                return false;
            }
            let run_now = if flow.running < observer.policy.max_in_flight && flow.queue.is_empty() {
                flow.running += 1;
                Some(PendingActivation::running(completed.clone()))
            } else if flow.queue.len() < observer.policy.max_queue
                && self.queue_budget.charge(completed)
            {
                flow.queue.push_back(PendingActivation::queued(
                    completed.clone(),
                    self.queue_budget.clone(),
                ));
                None
            } else {
                drop(flow);
                give_back();
                if let Some((cost, limit)) = self.queue_budget.too_large(completed) {
                    drop_too_large(observer, cost, limit, TooLarge::Queued);
                    return false;
                }
                drop_event(observer, observe_codes::OVERFLOW);
                return true;
            };
            let fired = taken.unwrap_or_else(|| observer.fires.fetch_add(1, Ordering::AcqRel) + 1);
            (run_now, fired)
        };
        if let Some(pending) = run_now {
            match self.runtime() {
                Some(runtime) => {
                    runtime.spawn(run_activations(self.clone(), observer.clone(), pending));
                }
                None => self.release_slot(observer),
            }
        }
        if observer.policy.max_fires == Some(fired) {
            self.dispose(
                observer,
                observe_codes::MAX_FIRES,
                format!("the observer fired {fired} times, its max_fires"),
            );
        }
        false
    }

    /// Invokes `observer`'s target with one activation, fire-and-forget for
    /// the observed side: the result is discarded. Bounded by
    /// `observers.activation_timeout_ms`, so a target that never answers does
    /// not hold the observer's in-flight slot forever; a target that
    /// disconnects mid-activation ends it at once (its invocations are
    /// halted). Either way the failure is counted.
    async fn invoke_activation(&self, observer: &Arc<Observer>, pending: PendingActivation) {
        let Some(deps) = self.deps.get() else {
            return;
        };
        let Some(invocations) = deps.invocations.upgrade() else {
            return;
        };
        let (target_namespace, target_function) = &*observer.target;
        let Some((handler, encoded_handler)) = deps
            .functions
            .get_handlers(target_namespace, target_function)
        else {
            count_failure(observer, failure_reasons::TARGET_UNAVAILABLE);
            return;
        };
        // One atomic add: `iii.observer.activations.total` follows from the
        // counters (see [`export_drops`]).
        observer
            .counters()
            .activations
            .count(pending.event.delivery.label());
        // Built now, from the event the observers share; the event is let go
        // (and its share of the queue budget given back) before the target
        // answers.
        let mut event = pending.event.clone();
        drop(pending);
        let activation_id = super::new_uuid().to_string();
        // A target a worker connection hosts gets the event's payloads as
        // the JSON text every activation of the event shares; any other gets
        // its own value, moved out of the event by its last holder (CONTRACT
        // D3).
        let dispatch = match encoded_handler {
            Some(handler) => Dispatch::Encoded {
                handler,
                body: project_encoded(observer, &event, &activation_id),
            },
            None => Dispatch::Value {
                handler,
                body: project_owned(observer, &mut event, &activation_id),
            },
        };
        let ctx = InvocationCtx::activation(
            Caller {
                worker_name: Some(ENGINE_CALLER_NAME.to_string()),
                function_id: Some(event.function.1.clone()),
                invocation_id: Some(event.invocation_id.to_string()),
                ..Caller::default()
            },
            event.depth.saturating_add(1),
            Activation {
                observer_id: observer.id.clone(),
                activation_id: Some(activation_id),
            },
        );
        let (traceparent, baggage) = (
            event.traceparent.as_deref().map(String::from),
            event.baggage.as_deref().map(String::from),
        );
        drop(event);
        let invocation_id = super::new_uuid();
        let call = invocations.handle_activation(
            invocation_id,
            target_function.clone(),
            target_namespace,
            dispatch,
            traceparent,
            baggage,
            observer.observing_session(),
            observer.metadata.clone().map(Arc::new),
            ctx,
        );
        let timeout = Duration::from_millis(self.config.load().activation_timeout_ms);
        let outcome = tokio::time::timeout(timeout, call).await;
        if outcome.is_err() {
            self.abandon(&observer.target, invocation_id).await;
        }
        if self.not_sent_too_large(observer, invocation_id, &outcome) {
            return;
        }
        if let Some(reason) = failure_of(&outcome) {
            tracing::debug!(
                observer_id = %observer.id,
                target = %target_function,
                reason,
                "activation failed"
            );
            count_failure(observer, reason);
        }
    }

    /// Notes that the engine did not send invocation `invocation_id` to its
    /// target worker: the serialized envelope (`size` bytes) is larger than
    /// the worker accepts (`limit`). The caller gets `payload_too_large`; when
    /// that caller is an activation, the event never reached the target and
    /// counts as a `TOO_LARGE` drop rather than the target's error (a target
    /// answering `payload_too_large` itself, because its result was too large,
    /// is still `TARGET_ERROR`). Only the last [`REFUSED_SENDS_KEPT`] are
    /// kept: noting one costs nothing on the path of calls that fit.
    pub(crate) fn refused_send(&self, invocation_id: Uuid, size: usize, limit: usize) {
        let mut refused = lock(&self.refused_sends);
        if refused.len() == REFUSED_SENDS_KEPT {
            refused.pop_front();
        }
        refused.push_back(RefusedSend {
            invocation_id,
            size,
            limit,
        });
    }

    /// Counts a `TOO_LARGE` drop when `outcome` is the engine refusing to send
    /// the activation `invocation_id` (see [`Self::refused_send`]).
    fn not_sent_too_large(
        &self,
        observer: &Observer,
        invocation_id: Uuid,
        outcome: &ActivationOutcome,
    ) -> bool {
        let Ok(Ok(Err(error))) = outcome else {
            return false;
        };
        if error.code != PAYLOAD_TOO_LARGE {
            return false;
        }
        let refused = {
            let mut refused = lock(&self.refused_sends);
            refused
                .iter()
                .rposition(|r| r.invocation_id == invocation_id)
                .and_then(|at| refused.remove(at))
        };
        let Some(refused) = refused else {
            return false;
        };
        drop_too_large(observer, refused.size, refused.limit, TooLarge::Sent);
        true
    }

    /// After a timeout: the target's connection stops tracking the
    /// invocation. A late answer finds nobody waiting and is discarded, as
    /// for any caller that gave up.
    async fn abandon(&self, target: &FnKey, invocation_id: Uuid) {
        let Some(deps) = self.deps.get() else {
            return;
        };
        let owner = deps.function_owners.get(target).map(|owner| owner.0);
        if let Some(worker) = owner.and_then(|id| deps.workers.get_worker(&id)) {
            worker.remove_invocation(&invocation_id).await;
        }
    }

    /// The next queued activation, keeping the slot; or `None`, freeing it.
    fn next_queued(&self, observer: &Observer) -> Option<PendingActivation> {
        let mut flow = observer.flow.write();
        match flow.queue.pop_front() {
            Some(pending) => {
                if flow.queue.len() == observer.policy.max_queue / 2 {
                    self.queue_budget.room.notify_waiters();
                }
                Some(pending)
            }
            None => {
                flow.running = flow.running.saturating_sub(1);
                if flow.queue.capacity() > QUEUE_KEPT {
                    flow.queue.shrink_to(QUEUE_KEPT);
                }
                None
            }
        }
    }

    /// Frees one of `observer`'s in-flight slots, or hands it to the next
    /// queued activation.
    fn release_slot(self: &Arc<Self>, observer: &Arc<Observer>) {
        if let Some(pending) = self.next_queued(observer) {
            match self.runtime() {
                Some(runtime) => {
                    runtime.spawn(run_activations(self.clone(), observer.clone(), pending));
                }
                None => {
                    let mut flow = observer.flow.write();
                    flow.running = flow.running.saturating_sub(1);
                }
            }
        }
    }

    /// `invocationsobserved` from `connection` (protocol.md §3.4): local
    /// calls of the functions it hosts, with the observers its delivery
    /// matched. The reporter must host each item's function; each `matched`
    /// id must be an active observer of it whose `on` takes the call's
    /// status. The engine does not re-evaluate filters: the host is
    /// authoritative for its own function's calls. An item with
    /// `output_is_input` carries its value once, as `input`, which is also
    /// its `output`. Observers receive `input` and `output` without the
    /// engine's `_caller_worker_id` stamp. The payloads stay the JSON text
    /// the item carried ([`RawJson`]): a target a worker connection hosts
    /// receives that text, any other the value it decodes to.
    ///
    /// A routed call's host names its observers on the call's answer
    /// (`invocationresult.matched`) only: an item for a call the engine
    /// routed to the reporting connection is dropped as `INVALID_REPORT`
    /// (decision 38), while the engine waits on it and, once it is
    /// answered, while `answered` (the connection's) keeps it.
    ///
    /// Returns the observers whose queue was full for some item, for the
    /// connection's reader to wait on ([`Self::wait_for_room`]); not those
    /// whose target the reporting connection hosts, since its answers come
    /// in on the connection that would wait.
    #[must_use = "the reader waits for room in the queues a report overflowed"]
    pub fn report(
        self: &Arc<Self>,
        connection: Uuid,
        connection_namespace: &str,
        answered: &RoutedAnswers,
        items: ObservedItems,
    ) -> Vec<Arc<Observer>> {
        let mut full: Vec<Arc<Observer>> = Vec::new();
        if !items.rejected.is_empty() {
            tracing::warn!(
                connection = %connection,
                rejected = items.rejected.len(),
                first_reason = %items.rejected[0].reason,
                "invocationsobserved: items that do not decode were dropped"
            );
            for rejected in &items.rejected {
                count_function_drop(
                    rejected.function_id.as_deref().unwrap_or("<undecodable>"),
                    INVALID_REPORT,
                    1,
                );
            }
        }
        let worker_name = self
            .deps
            .get()
            .and_then(|deps| deps.workers.get_worker_name(&connection));
        let invocations = self.deps.get().and_then(|deps| deps.invocations.upgrade());

        for item in items.items {
            let ObservedItem {
                function_id,
                namespace,
                invocation_id,
                matched,
                status,
                input,
                output,
                error,
                caller,
                metadata,
                started_at_ms,
                duration_ms,
                observe_depth,
                traceparent,
                baggage,
                output_is_input,
            } = item;
            let key: FnKey = (
                namespace.unwrap_or_else(|| connection_namespace.to_string()),
                function_id,
            );
            let mut seen = HashSet::new();
            let matched: Vec<String> = matched
                .into_iter()
                .filter(|id| seen.insert(id.clone()))
                .collect();
            if self.owner_of(&key).map(|owner| owner.connection) != Some(connection) {
                self.drop_matched(&key, &matched, observe_codes::REPORTER_NOT_OWNER);
                continue;
            }
            let parsed = Uuid::parse_str(&invocation_id).ok();
            let routed = parsed.is_some_and(|id| {
                answered.contains(&id)
                    || invocations
                        .as_ref()
                        .is_some_and(|invocations| invocations.is_pending(&id, &key.1))
            });
            if routed {
                self.drop_matched(&key, &matched, INVALID_REPORT);
                continue;
            }
            let success = match status {
                InvocationStatus::Success => true,
                InvocationStatus::Error => false,
                InvocationStatus::Unknown(_) => {
                    self.drop_matched(&key, &matched, INVALID_REPORT);
                    continue;
                }
            };
            if let Some(metrics) = super::metrics() {
                metrics
                    .observer_reports_total
                    .add(1, &[KeyValue::new("function_id", key.1.clone())]);
            }
            // The reporting connection is where the invocation ran: its
            // identity is stamped, whatever the item says.
            let mut caller = caller.unwrap_or_default();
            caller.worker_id = Some(connection.to_string());
            caller.worker_name = worker_name.clone();
            // The payloads stay the text the host sent: a worker's activation
            // sends it as is, a target that takes a value decodes it, once
            // for all of the event's activations. `output_is_input` holds
            // that text once, as the input.
            let text = |raw: RawJson| Payload::text(raw.into_shared());
            let observed = |raw: RawJson| Payload::text(unstamped_text(raw.into_shared()));
            let input = input.map(observed);
            let output = if output_is_input {
                None
            } else {
                output.map(observed)
            };
            // The function's key as its active set holds it, shared with the
            // event; moved into one of its own only when none of its
            // observers is active.
            let key = match self.watch(&key.0, &key.1) {
                Some(set) => set.function.clone(),
                None => Arc::new(key),
            };
            let completed = Arc::new(Completed {
                delivery: Delivery::Host,
                function: key.clone(),
                invocation_id: InvocationId::reported(invocation_id, parsed),
                success,
                output_is_input,
                input,
                output,
                error: error.map(text),
                caller: serde_json::to_value(&caller).ok().map(Arc::new),
                metadata: metadata.map(text),
                started_at_ms,
                duration_ms,
                depth: observe_depth,
                traceparent: traceparent.map(String::into_boxed_str),
                baggage: baggage.map(String::into_boxed_str),
                queues: AtomicUsize::new(0),
                charged: AtomicUsize::new(0),
                encoded: OnceBox::new(),
            });
            for id in &matched {
                match self.get(id).filter(|o| o.function == key) {
                    Some(observer) if !observer.capture.fires(success) => {}
                    Some(observer) => {
                        if self.activate(&observer, &completed)
                            && !full.iter().any(|o| Arc::ptr_eq(o, &observer))
                            && !self.hosts_target(connection, &observer)
                        {
                            full.push(observer);
                        }
                    }
                    None => count_function_drop(&key.1, observe_codes::INACTIVE, 1),
                }
            }
        }
        full
    }

    /// Whether `connection` hosts `observer`'s target.
    fn hosts_target(&self, connection: Uuid, observer: &Observer) -> bool {
        self.deps
            .get()
            .and_then(|deps| deps.function_owners.get(&*observer.target).map(|o| o.0))
            == Some(connection)
    }

    /// Backpressure on a reporting connection (protocol.md §3.4). After a
    /// report overflowed `full`, the connection's reader waits here until
    /// each of those queues, and the queues together ([`QUEUED_BYTES`]), are
    /// down to half their limit (or the observer is no longer active), for
    /// [`REPORT_BACKPRESSURE_WAIT`] at most. Meanwhile
    /// the engine reads nothing more from that connection: a worker that
    /// reports faster than the targets take is slowed by its own socket, and
    /// its SDK drops the excess from its bounded report buffer
    /// (`BUFFER_FULL`), where it costs the engine nothing, instead of the
    /// engine decoding every report to drop it as `OVERFLOW`.
    ///
    /// Everything else the worker sends waits behind its reports, so `hold`
    /// bounds how much of the time it is held: not again until the reader
    /// has read for a quarter as long as it waited (see [`ReportHold`]).
    pub async fn wait_for_room(&self, hold: &mut ReportHold, full: &[Arc<Observer>]) {
        let started = tokio::time::Instant::now();
        if hold.not_before.is_some_and(|at| started < at) {
            return;
        }
        let deadline = started + REPORT_BACKPRESSURE_WAIT;
        let mut waited = false;
        'full: for observer in full {
            loop {
                let room = self.queue_budget.room.notified();
                tokio::pin!(room);
                room.as_mut().enable();
                let queued = observer.flow.write().queue.len();
                if !observer.is_active()
                    || (queued <= observer.policy.max_queue / 2 && self.queue_budget.has_room())
                {
                    break;
                }
                if !waited {
                    waited = true;
                    self.report_waits.fetch_add(1, Ordering::Relaxed);
                }
                if tokio::time::timeout_at(deadline, room).await.is_err() {
                    break 'full;
                }
            }
        }
        if waited {
            let ended = tokio::time::Instant::now();
            hold.not_before = Some(ended + (ended - started) / REPORT_HOLD_PER_READ);
        }
    }

    /// Charges a dropped report item to the observers it named (or to the
    /// function, for ids that are not its observers).
    fn drop_matched(&self, key: &FnKey, matched: &[String], reason: &str) {
        if matched.is_empty() {
            count_function_drop(&key.1, reason, 1);
        }
        for id in matched {
            match self.get(id).filter(|o| *o.function == *key) {
                Some(observer) => drop_event(&observer, reason),
                None => count_function_drop(&key.1, reason, 1),
            }
        }
    }

    /// A call through an observer (v2 §4.6): an `InvokeFunction` from
    /// `worker` carrying `activation`. The observer must exist, `worker` must
    /// host its function and it must be active; then its policy applies and
    /// the call is routed to the observer's stored target (the request's
    /// `function_id` and `namespace` are not read) with the observer's
    /// metadata, a fresh `activation_id` and depth + 1. It runs as the
    /// observer's activations do: under the observing worker's session, and
    /// its payload goes as given less a top-level `_caller_worker_id`
    /// (decision 49). The host is its `caller`. The result, or the refusal
    /// code, goes back to the host.
    ///
    /// A refusal by policy of a call the host waits on counts a drop as it
    /// does for an activation (`LOOP_GUARD`, `MAX_FIRES`, `OVERFLOW`,
    /// `INACTIVE` once expired). A fire-and-forget call nobody waits on
    /// counts every refusal under the refusal's code, and each is logged at
    /// `debug` (decision 13).
    pub async fn observer_call(
        self: &Arc<Self>,
        worker: &WorkerConnection,
        call: ObserverCall,
    ) -> Result<Option<Value>, ErrorBody> {
        let admitted = self
            .check_observer_call(worker.id, &call)
            .and_then(|observer| {
                let depth = call.observe_depth.unwrap_or(0);
                self.admit_observer_call(&observer, depth, call.fire_and_forget)
                    .map(|slot| (observer, slot, depth))
            });
        let (observer, _slot, depth) = match admitted {
            Ok(admitted) => admitted,
            Err(refusal) => {
                if call.fire_and_forget {
                    tracing::debug!(
                        worker_id = %worker.id,
                        observer_id = %call.activation.observer_id,
                        code = %refusal.code,
                        reason = %refusal.message,
                        "fire-and-forget call through an observer refused; dropped"
                    );
                }
                return Err(refusal);
            }
        };
        self.route_observer_call(worker, &observer, call, depth)
            .await
    }

    /// The checks before policy: the observer exists, `worker` hosts its
    /// function and it is active. A refused fire-and-forget call counts a
    /// drop under the refusal's code: in the observer's counters, or, for
    /// an observer the engine does not know, in the metric alone.
    fn check_observer_call(
        &self,
        worker: Uuid,
        call: &ObserverCall,
    ) -> Result<Arc<Observer>, ErrorBody> {
        let observer_id = &call.activation.observer_id;
        let refuse = |observer: Option<&Observer>, code: &str, message: String| {
            if call.fire_and_forget {
                match observer {
                    Some(observer) => drop_event(observer, code),
                    None => count_function_drop(UNKNOWN_FUNCTION, code, 1),
                }
            }
            Err(ErrorBody::new(code, message))
        };
        let Some(observer) = self.get(observer_id) else {
            return refuse(
                None,
                observe_codes::OBSERVER_NOT_FOUND,
                format!("no observer `{observer_id}`"),
            );
        };
        if self
            .owner_of(&observer.function)
            .map(|owner| owner.connection)
            != Some(worker)
        {
            return refuse(
                Some(&observer),
                observe_codes::NOT_FUNCTION_OWNER,
                format!(
                    "only the worker hosting `{}` calls through its observers",
                    observer.function.1
                ),
            );
        }
        if !observer.is_active() {
            return refuse(
                Some(&observer),
                observe_codes::OBSERVER_NOT_ACTIVE,
                format!("observer `{}` is not active", observer.id),
            );
        }
        Ok(observer)
    }

    /// Policy for a call through `observer`, in order: depth, expiry,
    /// `max_fires`, the state again under the flow lock, `max_in_flight` (no
    /// queue: the host is waiting for the result). Returns the in-flight
    /// slot the call holds. A refusal counts a drop under the policy's
    /// reason, or under the refusal's code for a `fire_and_forget` call; an
    /// observer disposed since `check_observer_call` refuses as that check
    /// does.
    fn admit_observer_call(
        self: &Arc<Self>,
        observer: &Arc<Observer>,
        depth: u32,
        fire_and_forget: bool,
    ) -> Result<Slot, ErrorBody> {
        let refuse = |policy: &str, code: &str, message: String| {
            drop_event(observer, if fire_and_forget { code } else { policy });
            Err(ErrorBody::new(code, message))
        };
        let max_depth = self.config().max_observe_depth;
        if depth.saturating_add(1) > max_depth {
            warn_loop_guard(observer, depth, max_depth);
            return refuse(
                observe_codes::LOOP_GUARD,
                observe_codes::LOOP_GUARD,
                format!("the chain of observers is deeper than {max_depth}"),
            );
        }
        if observer.is_expired() {
            self.dispose(
                observer,
                observe_codes::EXPIRED,
                "the observer's expires_in_ms elapsed",
            );
            return refuse(
                observe_codes::INACTIVE,
                observe_codes::OBSERVER_NOT_ACTIVE,
                format!("observer `{}` expired", observer.id),
            );
        }
        let Some(fired) = try_fire(observer) else {
            return refuse(
                observe_codes::MAX_FIRES,
                observe_codes::OBSERVER_NOT_ACTIVE,
                format!("observer `{}` reached its max_fires", observer.id),
            );
        };
        {
            let mut flow = observer.flow.write();
            // The observer was active in `check_observer_call`; a disposal
            // since then refuses the call, as it does for an activation. A
            // `MAX_FIRES` disposal keeps this fire, which is within
            // `max_fires`.
            if !observer.runs_counted_fires() {
                drop(flow);
                observer.fires.fetch_sub(1, Ordering::AcqRel);
                if fire_and_forget {
                    drop_event(observer, observe_codes::OBSERVER_NOT_ACTIVE);
                }
                return Err(ErrorBody::new(
                    observe_codes::OBSERVER_NOT_ACTIVE,
                    format!("observer `{}` is not active", observer.id),
                ));
            }
            if flow.running >= observer.policy.max_in_flight {
                drop(flow);
                observer.fires.fetch_sub(1, Ordering::AcqRel);
                return refuse(
                    observe_codes::OVERFLOW,
                    observe_codes::OVERFLOW,
                    format!(
                        "observer `{}` already has {} calls in flight",
                        observer.id, observer.policy.max_in_flight
                    ),
                );
            }
            flow.running += 1;
        }
        let slot = Slot {
            registry: self.clone(),
            observer: observer.clone(),
            held: true,
        };
        if observer.policy.max_fires == Some(fired) {
            self.dispose(
                observer,
                observe_codes::MAX_FIRES,
                format!("the observer fired {fired} times, its max_fires"),
            );
        }
        Ok(slot)
    }

    async fn route_observer_call(
        &self,
        worker: &WorkerConnection,
        observer: &Arc<Observer>,
        call: ObserverCall,
        depth: u32,
    ) -> Result<Option<Value>, ErrorBody> {
        let Some(deps) = self.deps.get() else {
            return Err(ErrorBody::new(
                "invocation_error",
                "the engine is shutting down",
            ));
        };
        let Some(invocations) = deps.invocations.upgrade() else {
            return Err(ErrorBody::new(
                "invocation_error",
                "the engine is shutting down",
            ));
        };
        let (target_namespace, target_function) = &*observer.target;
        let Some(handler) = deps
            .functions
            .get_handler(target_namespace, target_function)
        else {
            count_failure(observer, failure_reasons::TARGET_UNAVAILABLE);
            return Err(ErrorBody::new(
                "function_not_found",
                format!("Function {target_function} not found in namespace {target_namespace}."),
            ));
        };
        if let Some(metrics) = super::metrics() {
            metrics.observer_activations_total.add(
                1,
                &[
                    KeyValue::new("function_id", observer.function.1.clone()),
                    KeyValue::new("delivery", OBSERVER_CALL_DELIVERY),
                ],
            );
        }
        let ctx = InvocationCtx::observer_call(
            worker.id,
            deps.workers.clone(),
            call.caller,
            depth.saturating_add(1),
            Activation {
                observer_id: observer.id.clone(),
                activation_id: Some(Uuid::new_v4().to_string()),
            },
        );
        let invocation_id = call.invocation_id.unwrap_or_else(Uuid::new_v4);
        // The target runs as for the observer's activations: under the
        // observing worker's session (none for an engine-owned observer),
        // with no `_caller_worker_id` in its payload, and a private target
        // checks the observing connection. The host chose the payload and
        // the observing worker chose the target, so the host's authority
        // never reaches the target (decisions 49, 57).
        let observing = observer.observing();
        let invocation = invocations.handle_observer_call(
            invocation_id,
            worker,
            (!observer.is_engine_owned()).then_some(observing.connection),
            target_function.clone(),
            target_namespace,
            unstamped(call.data),
            handler,
            call.traceparent,
            call.baggage,
            observing.session,
            observer.metadata.clone().map(Arc::new),
            ctx,
        );
        let timeout = Duration::from_millis(self.config().activation_timeout_ms);
        let outcome = tokio::time::timeout(timeout, invocation).await;
        // The host going away halts its calls, this one included: that is
        // not the target's failure.
        let host_gone = worker
            .cleanup_claimed
            .load(std::sync::atomic::Ordering::SeqCst);
        if outcome.is_err() || host_gone {
            self.abandon(&observer.target, invocation_id).await;
        }
        // Tracked on the host's connection whether or not it gets an answer
        // (an `action: void` call does not).
        worker.remove_invocation(&invocation_id).await;
        match failure_of(&outcome) {
            Some(failure_reasons::TARGET_UNAVAILABLE) if host_gone => {}
            // Not sent: the observer counts a drop, and the host still gets
            // the `payload_too_large` error below.
            Some(_) if self.not_sent_too_large(observer, invocation_id, &outcome) => {}
            Some(reason) => count_failure(observer, reason),
            None => {}
        }
        match outcome {
            Ok(Ok(result)) => result,
            Ok(Err(err)) => Err(ErrorBody::new("invocation_error", err.to_string())),
            Err(_) => Err(ErrorBody::new(
                "TIMEOUT",
                format!(
                    "the target `{target_function}` did not answer within \
                     observers.activation_timeout_ms"
                ),
            )),
        }
    }

    /// `localinvocationstats` from `connection`: local invocation counts
    /// (otherwise invisible to the engine's invocation metrics) and the SDK's
    /// own report drops. Only for functions the connection owns.
    pub fn local_stats(
        &self,
        connection: Uuid,
        connection_namespace: &str,
        functions: &[LocalFunctionStats],
        dropped_reports: &[DroppedReportStats],
    ) {
        let owns = |function_id: &str| {
            let key = (connection_namespace.to_string(), function_id.to_string());
            self.owner_of(&key).map(|owner| owner.connection) == Some(connection)
        };
        let acc = crate::workers::observability::metrics::get_metrics_accumulator();
        for stats in functions {
            if !owns(&stats.function_id) {
                tracing::debug!(
                    connection = %connection,
                    function_id = %stats.function_id,
                    "localinvocationstats for a function the connection does not own; ignoring"
                );
                continue;
            }
            let errors = stats.errors.min(stats.count);
            let ok = stats.count - errors;
            if let Some(metrics) = super::metrics() {
                for (status, count) in [("ok", ok), ("error", errors)] {
                    if count > 0 {
                        metrics.invocations_total.add(
                            count,
                            &[
                                KeyValue::new("function_id", stats.function_id.clone()),
                                KeyValue::new("status", status),
                                KeyValue::new("dispatch", "local"),
                            ],
                        );
                    }
                }
            }
            for (counter, count) in [
                (&acc.invocations_total, stats.count),
                (&acc.invocations_success, ok),
                (&acc.invocations_error, errors),
            ] {
                saturating_add(counter, count);
            }
            acc.add_function_invocations(&stats.function_id, stats.count);
        }
        for dropped in dropped_reports {
            if owns(&dropped.function_id) {
                count_function_drop(
                    &dropped.function_id,
                    sdk_drop_reason(&dropped.reason),
                    dropped.count,
                );
            }
        }
    }
}

/// A host's call through an observer, as it arrived.
pub struct ObserverCall {
    pub invocation_id: Option<Uuid>,
    pub data: Value,
    pub caller: Option<Box<Caller>>,
    pub observe_depth: Option<u32>,
    pub activation: Activation,
    pub traceparent: Option<String>,
    pub baggage: Option<String>,
    /// Nobody waits for the answer (`action: void`, or no `invocation_id`).
    pub fire_and_forget: bool,
}

/// An SDK-side drop reason as a metric label: the reasons SDKs report
/// (protocol.md §4), anything else as `OTHER`, so a peer cannot mint labels.
fn sdk_drop_reason(reason: &str) -> &'static str {
    [
        observe_codes::BUFFER_FULL,
        observe_codes::TOO_LARGE,
        observe_codes::DISCONNECTED,
    ]
    .into_iter()
    .find(|known| reason.eq_ignore_ascii_case(known))
    .unwrap_or("OTHER")
}

fn saturating_add(counter: &std::sync::atomic::AtomicU64, count: u64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |total| {
        Some(total.saturating_add(count))
    });
}

/// Counts one fire against `max_fires`; `None` (and a `MAX_FIRES` drop) when
/// the observer has none left. Returns the fire's number.
fn take_fire(observer: &Observer) -> Option<u64> {
    let fired = try_fire(observer);
    if fired.is_none() {
        drop_event(observer, observe_codes::MAX_FIRES);
    }
    fired
}

/// Counts one fire against `max_fires`; `None` when the observer has none
/// left. Returns the fire's number.
fn try_fire(observer: &Observer) -> Option<u64> {
    match observer.policy.max_fires {
        Some(max) => observer
            .fires
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |fires| {
                (fires < max).then_some(fires + 1)
            })
            .ok()
            .map(|previous| previous + 1),
        None => Some(observer.fires.fetch_add(1, Ordering::AcqRel) + 1),
    }
}

/// An activation's result, as its invoker sees it.
type ActivationOutcome =
    Result<Result<Result<Option<Value>, ErrorBody>, RecvError>, tokio::time::error::Elapsed>;

/// The code the engine answers a call with when it does not send the call
/// because the target worker cannot receive it (`Engine::guard_outbound`).
const PAYLOAD_TOO_LARGE: &str = "payload_too_large";

/// An invocation the engine did not send (see
/// [`ObserverRegistry::refused_send`]).
pub(crate) struct RefusedSend {
    invocation_id: Uuid,
    size: usize,
    limit: usize,
}

/// Why an activation failed, if it did.
fn failure_of(outcome: &ActivationOutcome) -> Option<&'static str> {
    match outcome {
        Err(_) => Some(failure_reasons::TIMEOUT),
        Ok(Ok(Ok(_))) => None,
        Ok(Err(_)) => Some(failure_reasons::TARGET_UNAVAILABLE),
        Ok(Ok(Err(error)))
            if error.code == "invocation_stopped" || error.code == "function_not_found" =>
        {
            Some(failure_reasons::TARGET_UNAVAILABLE)
        }
        Ok(Ok(Err(_))) => Some(failure_reasons::TARGET_ERROR),
    }
}

fn count_failure(observer: &Observer, reason: &str) {
    observer.counters().failures.count(reason);
    if let Some(metrics) = super::metrics() {
        metrics.observer_activation_failures_total.add(
            1,
            &[
                KeyValue::new("function_id", observer.function_label()),
                KeyValue::new("reason", reason.to_string()),
            ],
        );
    }
}

/// A dropped event that no observer can be charged with (the matched id is
/// not one of the function's observers, the item did not decode).
fn count_function_drop(function_id: &str, reason: &str, count: u64) {
    if let Some(metrics) = super::metrics() {
        metrics.observer_dropped_total.add(
            count,
            &[
                KeyValue::new("function_id", function_id.to_string()),
                KeyValue::new("reason", reason.to_string()),
            ],
        );
    }
}

/// Runs `first`, then whatever queued behind it while the slot was held, in
/// arrival order; releases the slot when the queue is empty.
async fn run_activations(
    registry: Arc<ObserverRegistry>,
    observer: Arc<Observer>,
    first: PendingActivation,
) {
    let mut slot = Slot {
        registry: registry.clone(),
        observer: observer.clone(),
        held: true,
    };
    let mut next = first;
    loop {
        registry.invoke_activation(&observer, next).await;
        match registry.next_queued(&observer) {
            Some(pending) => next = pending,
            None => {
                slot.held = false;
                return;
            }
        }
    }
}

/// One payload of an event, as [`Parts`] reads it.
#[derive(Clone, Copy)]
enum Part {
    Input,
    Output,
    Error,
    Caller,
    Metadata,
}

impl Payload {
    /// Its value, owned: the value it holds, or the one decoded from its
    /// text (by an earlier activation, or now).
    fn into_value(self) -> Option<Arc<Value>> {
        match self {
            Payload::Value(value) => Some(value),
            Payload::Text(text) => {
                let PayloadText { text, value } = *text;
                match value.into_inner() {
                    Some(decoded) => decoded,
                    None => serde_json::from_str(&text).ok().map(Arc::new),
                }
            }
        }
    }
}

impl Completed {
    /// The input as a value, for a target that takes one (see
    /// [`Payload::value`]).
    #[cfg(test)]
    fn input_value(&self) -> Option<&Value> {
        self.input
            .as_ref()
            .and_then(Payload::value)
            .map(Arc::as_ref)
    }

    /// The output as a value: the input's when it is the input.
    #[cfg(test)]
    fn output_value(&self) -> Option<&Value> {
        if self.output_is_input {
            return self.input_value();
        }
        self.output
            .as_ref()
            .and_then(Payload::value)
            .map(Arc::as_ref)
    }

    /// `part` as a value shared with the event: a payload held as text is
    /// decoded once, for all of the event's activations that take a value
    /// (see [`Payload::value`]).
    fn shared_part(&self, part: Part) -> Option<Arc<Value>> {
        let payload = match part {
            Part::Input => &self.input,
            Part::Output if self.output_is_input => &self.input,
            Part::Output => &self.output,
            Part::Error => &self.error,
            Part::Caller => return self.caller.clone(),
            Part::Metadata => &self.metadata,
        };
        payload.as_ref().and_then(Payload::value).cloned()
    }

    /// `part` taken from an event nobody else holds any more (CONTRACT D3):
    /// moved rather than copied. A report's input that is its output too is
    /// shared by both.
    fn take_part(&mut self, part: Part) -> Option<Arc<Value>> {
        let payload = match part {
            Part::Input | Part::Output if self.output_is_input => return self.shared_part(part),
            Part::Input => &mut self.input,
            Part::Output => &mut self.output,
            Part::Error => &mut self.error,
            Part::Caller => return self.caller.take(),
            Part::Metadata => &mut self.metadata,
        };
        payload.take().and_then(Payload::into_value)
    }
}

/// The values an activation's payload carries, read only for the fields it
/// ships: still shared with whoever else holds the event, or taken from an
/// event nobody else holds (CONTRACT D3).
#[derive(Default)]
struct Parts {
    input: Option<Arc<Value>>,
    output: Option<Arc<Value>>,
    error: Option<Arc<Value>>,
    caller: Option<Arc<Value>>,
    metadata: Option<Arc<Value>>,
}

impl Parts {
    /// What `observer` ships of an event that succeeded or not: with
    /// `deliver`, the input or output alone; otherwise what it captures that
    /// applies to the outcome (`output` to a success, `error` to a failure,
    /// whatever a report carries). Each is read with `read`.
    fn read(
        observer: &Observer,
        success: bool,
        mut read: impl FnMut(Part) -> Option<Arc<Value>>,
    ) -> Self {
        let capture = observer.capture;
        let mut want = |wanted: bool, part: Part| if wanted { read(part) } else { None };
        match observer.deliver {
            Deliver::Input => Self {
                input: want(true, Part::Input),
                ..Self::default()
            },
            Deliver::Output => Self {
                output: want(true, Part::Output),
                ..Self::default()
            },
            Deliver::Payload => Self {
                input: want(capture.input, Part::Input),
                output: want(capture.output && success, Part::Output),
                error: want(capture.error && !success, Part::Error),
                caller: want(capture.caller, Part::Caller),
                metadata: want(capture.metadata, Part::Metadata),
            },
        }
    }
}

/// A part as the payload carries it: moved out when nothing else shares it
/// any more, copied otherwise.
fn ship(part: Option<Arc<Value>>) -> Option<Value> {
    part.map(Arc::unwrap_or_clone)
}

/// The target's first argument (README §4.3), every part copied: the event
/// stays whole.
#[cfg(test)]
fn project(observer: &Observer, completed: &Completed, activation_id: &str) -> Value {
    let parts = Parts::read(observer, completed.success, |part| {
        completed.shared_part(part)
    });
    payload(observer, completed, activation_id, parts)
}

/// The target's first argument for the activation holding `completed`, when
/// its target takes a value (CONTRACT D3). While other observers still hold
/// the event its parts are copied; the last holder takes them instead, so
/// the input, output, caller and metadata the call kept reach the target
/// without a copy. The event keeps every other field.
fn project_owned(
    observer: &Observer,
    completed: &mut Arc<Completed>,
    activation_id: &str,
) -> Value {
    let parts = match Arc::get_mut(completed) {
        Some(sole) => {
            let success = sole.success;
            Parts::read(observer, success, |part| sole.take_part(part))
        }
        None => Parts::read(observer, completed.success, |part| {
            completed.shared_part(part)
        }),
    };
    payload(observer, completed, activation_id, parts)
}

/// The payload (the top-level fields plus `parts`) or, for the engine's own
/// observers and an older SDK's `deliver: input`, the observed input alone;
/// for its `deliver: output`, the observed output alone, `null` when there is
/// none (on a failure too), as v0.25 did. A field with no value is left out.
fn payload(observer: &Observer, completed: &Completed, activation_id: &str, parts: Parts) -> Value {
    match observer.deliver {
        Deliver::Input => return ship(parts.input).unwrap_or(Value::Null),
        Deliver::Output => return ship(parts.output).unwrap_or(Value::Null),
        Deliver::Payload => {}
    }
    let mut envelope = Map::new();
    envelope.insert("observer_id".into(), observer.id.clone().into());
    envelope.insert("activation_id".into(), activation_id.into());
    envelope.insert("function_id".into(), completed.function.1.clone().into());
    envelope.insert("namespace".into(), completed.function.0.clone().into());
    envelope.insert(
        "invocation_id".into(),
        completed.invocation_id.to_string().into(),
    );
    envelope.insert(
        "status".into(),
        if completed.success {
            "success"
        } else {
            "error"
        }
        .into(),
    );
    for (key, value) in [
        ("input", parts.input),
        ("output", parts.output),
        ("error", parts.error),
        ("caller", parts.caller),
        ("metadata", parts.metadata),
    ] {
        if let Some(value) = ship(value) {
            envelope.insert(key.into(), value);
        }
    }
    envelope.insert("started_at_ms".into(), completed.started_at_ms.into());
    envelope.insert(
        "duration_ms".into(),
        serde_json::Number::from_f64(completed.duration_ms)
            .map(Value::Number)
            .unwrap_or(Value::Null),
    );
    Value::Object(envelope)
}

/// Reads one of a completion's payloads as JSON text.
type EncodedReader = fn(&Completed) -> Option<Arc<str>>;

/// [`project`] as JSON text, for a target a worker connection hosts: the
/// same value, with the event's payloads encoded once and shared by its
/// activations instead of copied into each.
fn project_encoded(observer: &Observer, completed: &Completed, activation_id: &str) -> EncodedJson {
    let shipped = |text: Option<Arc<str>>| match text {
        Some(text) => EncodedJson::shared(text),
        None => EncodedJson::of(&Value::Null),
    };
    match observer.deliver {
        Deliver::Input => return shipped(completed.encoded_input()),
        Deliver::Output => return shipped(completed.encoded_output()),
        Deliver::Payload => {}
    }
    let capture = observer.capture;
    let mut envelope = ObjectWriter::new();
    envelope.field("observer_id", &observer.id);
    envelope.field("activation_id", &activation_id);
    envelope.field("function_id", &completed.function.1);
    envelope.field("namespace", &completed.function.0);
    envelope.field("invocation_id", &completed.invocation_id);
    envelope.field(
        "status",
        &if completed.success {
            "success"
        } else {
            "error"
        },
    );
    // As [`payload`]: `output` applies to a success and `error` to a
    // failure, and a field with no value is left out.
    let fields: [(bool, &str, EncodedReader); 5] = [
        (capture.input, "input", Completed::encoded_input),
        (
            capture.output && completed.success,
            "output",
            Completed::encoded_output,
        ),
        (
            capture.error && !completed.success,
            "error",
            Completed::encoded_error,
        ),
        (capture.caller, "caller", Completed::encoded_caller),
        (capture.metadata, "metadata", Completed::encoded_metadata),
    ];
    for (wanted, key, text) in fields {
        if wanted && let Some(text) = text(completed) {
            envelope.shared_field(key, &text);
        }
    }
    envelope.field("started_at_ms", &completed.started_at_ms);
    // Not finite: `null`, as `project` writes it.
    envelope.field("duration_ms", &completed.duration_ms);
    envelope.finish()
}

/// Counts one dropped event for `observer` under `reason`: one atomic add.
/// `iii.observer.dropped.total` follows from the counters (see
/// [`ObserverRegistry::export_drops`]).
fn drop_event(observer: &Observer, reason: &str) {
    let drops = &observer.counters().drops;
    let counter = match reason {
        observe_codes::LOOP_GUARD => &drops.loop_guard,
        observe_codes::MAX_FIRES => &drops.max_fires,
        observe_codes::OVERFLOW => &drops.overflow,
        observe_codes::REPORTER_NOT_OWNER => &drops.reporter_not_owner,
        CANCELLED => &drops.cancelled,
        INVALID_REPORT => &drops.invalid_report,
        observe_codes::TOO_LARGE => &drops.too_large,
        observe_codes::NOT_FUNCTION_OWNER => &drops.not_function_owner,
        observe_codes::OBSERVER_NOT_ACTIVE => &drops.observer_not_active,
        _ => &drops.inactive,
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

/// The limit a `TOO_LARGE` event did not fit in.
#[derive(Debug, Clone, Copy)]
enum TooLarge {
    /// The bytes the engine holds for observers' evaluation
    /// ([`EVALUATOR_BYTES`]).
    Held,
    /// The observers' queues together ([`QUEUED_BYTES`]).
    Queued,
    /// What the target worker receives in one message.
    Sent,
}

impl TooLarge {
    /// What did not fit, and what to change.
    fn explain(self) -> &'static str {
        match self {
            Self::Held => {
                "what the function's observers capture or filter on is larger than the engine \
                 keeps for observers; capture fewer fields (input and output both count)"
            }
            Self::Queued => {
                "the event is larger than the observers' queues hold together; capture fewer \
                 fields (input and output both count)"
            }
            Self::Sent => {
                "the activation is larger than the target worker accepts; send the target less \
                 (capture fewer fields, or deliver the input or output alone) or pass large \
                 payloads through a channel"
            }
        }
    }
}

/// Counts a `TOO_LARGE` drop for `observer`: one event of about `size`
/// bytes that cannot fit `limit` even with nothing else waiting, so unlike
/// `OVERFLOW` it says nothing about load. Warns at most once a minute per
/// observer.
fn drop_too_large(observer: &Observer, size: usize, limit: usize, too_large: TooLarge) {
    drop_event(observer, observe_codes::TOO_LARGE);
    if !once_a_minute(&observer.counters().last_too_large_warn_ms) {
        return;
    }
    tracing::warn!(
        observer_id = %observer.id,
        function_id = %observer.function.1,
        target = %observer.target.1,
        size_bytes = size,
        limit_bytes = limit,
        "observer event not delivered (TOO_LARGE): {}",
        too_large.explain()
    );
}

/// Whether a warning last logged at `last` (ms) may be logged again: at
/// most once a minute.
fn once_a_minute(last: &AtomicU64) -> bool {
    let now = now_ms();
    let before = last.load(Ordering::Relaxed);
    now.saturating_sub(before) >= 60_000
        && last
            .compare_exchange(before, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
}

/// Adds what `observer` dropped since its last export to
/// `iii.observer.dropped.total`, and the activations it started to
/// `iii.observer.activations.total`.
pub(crate) fn export_drops(observer: &Observer) {
    let (Some(metrics), Some(counters)) = (super::metrics(), observer.counted()) else {
        return;
    };
    // Made once, and only when there is something to export.
    let label = std::cell::OnceCell::new();
    let function_id = || label.get_or_init(|| observer.function_label()).clone();
    for (reason, count) in counters.drops.unexported() {
        metrics.observer_dropped_total.add(
            count,
            &[
                KeyValue::new("function_id", function_id()),
                KeyValue::new("reason", reason),
            ],
        );
    }
    for (delivery, count) in counters.activations.unexported() {
        metrics.observer_activations_total.add(
            count,
            &[
                KeyValue::new("function_id", function_id()),
                KeyValue::new("delivery", delivery),
            ],
        );
    }
}

/// The evaluator that takes `set`'s completions.
fn evaluator_of<'a>(
    evaluators: &'a [mpsc::Sender<Evaluation>],
    set: &ActiveSet,
) -> &'a mpsc::Sender<Evaluation> {
    &evaluators[(set.shard % evaluators.len() as u64) as usize]
}

/// At most one warning per observer per minute: a loop fires as fast as it
/// can, and the counter already has the numbers.
fn warn_loop_guard(observer: &Observer, depth: u32, max_depth: u32) {
    if !once_a_minute(&observer.counters().last_loop_warn_ms) {
        return;
    }
    tracing::warn!(
        observer_id = %observer.id,
        function_id = %observer.function.1,
        target = %observer.target.1,
        depth,
        max_depth,
        "observer activation dropped by the loop guard: the chain of observers is deeper than \
         observers.max_observe_depth (a target that calls back into what it observes?)"
    );
}

#[cfg(test)]
#[path = "legacy_matrix_tests.rs"]
mod legacy_matrix_tests;

#[cfg(test)]
mod metadata_tests;

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use serde_json::{Value, json};
    use uuid::Uuid;

    use super::super::{InvocationCtx, events::test_support};
    use super::{ROUTED_ANSWERS_KEPT, RoutedAnswers};
    use crate::{
        engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
        function::FunctionResult,
        protocol::{DEFAULT_NAMESPACE, ObserverState, observe_codes},
    };

    fn source(engine: &Engine) {
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::src".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|input: Value| async move { FunctionResult::Success(Some(input)) }),
        );
    }

    /// The routed hook hands a completion over and returns: evaluation runs
    /// later, on the evaluator task, so the caller's reply never waits for it.
    #[tokio::test]
    async fn completing_an_observed_call_does_not_evaluate_it_inline() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let _socket = test_support::observe(&engine, "obs", "test::src", "test::dst", None).await;
        let registry = engine.observers.clone();
        let set = registry
            .watch(DEFAULT_NAMESPACE, "test::src")
            .expect("observed");
        let ctx = Arc::new(InvocationCtx::engine_internal_at(0));
        let call = registry.begin(set, &json!({ "n": 1 }), &None, &None, &None);
        let before = registry.evaluated();
        registry.complete(
            call,
            DEFAULT_NAMESPACE,
            "test::src",
            Uuid::new_v4(),
            &Ok(Ok(Some(json!(true)))),
            None,
            &ctx,
        );
        assert_eq!(registry.evaluated(), before, "not evaluated inline");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while registry.evaluated() == before {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("evaluated on the evaluator task");
    }

    /// An observed invocation cancelled before its result (its caller gave
    /// up) is counted, not silently lost.
    #[tokio::test]
    async fn a_cancelled_observed_call_counts_a_drop() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let _socket = test_support::observe(&engine, "obs", "test::src", "test::dst", None).await;
        let set = engine
            .observers
            .watch(DEFAULT_NAMESPACE, "test::src")
            .expect("observed");
        drop(engine.observers.begin(set, &json!({}), &None, &None, &None));
        let info = engine.observers.get_info(&engine, None, "obs").unwrap();
        assert_eq!(info.dropped.get("CANCELLED"), Some(&1));
    }

    /// A target that panics while handling an activation does not keep the
    /// observer's in-flight slot: the next activation runs.
    #[tokio::test]
    async fn a_target_that_panics_frees_its_slot() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let calls = Arc::new(AtomicUsize::new(0));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        {
            let calls = calls.clone();
            engine.register_function_handler(
                RegisterFunctionRequest {
                    function_id: "test::panicky".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                Handler::new(move |input: Value| {
                    let calls = calls.clone();
                    let tx = tx.clone();
                    async move {
                        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                            panic!("the first activation's handler panics");
                        }
                        let _ = tx.send(input);
                        FunctionResult::Success(None)
                    }
                }),
            );
        }
        let (worker, _socket) = test_support::worker(&engine);
        let mut request = test_support::request("obs", "test::src", "test::panicky", None);
        request.max_in_flight = Some(1);
        test_support::observe_request(&engine, &worker, request).await;

        for n in 1..=2 {
            engine.call("test::src", json!({ "n": n })).await.unwrap();
        }
        let delivered = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("the second activation ran")
            .unwrap();
        assert_eq!(delivered, json!({ "n": 2 }));
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while engine.observers.in_flight("obs") != Some((0, 0)) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .is_ok()
        );
    }

    /// A call through an observer that policy refuses (expired before its
    /// timer disposed it, or its last fire taken meanwhile) is refused with
    /// `OBSERVER_NOT_ACTIVE`. Fire-and-forget, it counts that code as its
    /// drop reason (decision 13); awaited, the policy's reason, as an
    /// activation does.
    #[tokio::test]
    async fn a_refused_fire_and_forget_call_counts_the_refusal_code() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let (host, _socket) = test_support::worker(&engine);
        engine.function_owners.insert(
            (DEFAULT_NAMESPACE.to_string(), "test::src".to_string()),
            (host.id, crate::engine::InvocationKind::Regular),
        );
        let call = |observer_id: &str, fire_and_forget: bool| super::ObserverCall {
            invocation_id: None,
            data: json!({}),
            caller: None,
            observe_depth: None,
            activation: crate::protocol::Activation {
                observer_id: observer_id.to_string(),
                activation_id: None,
            },
            traceparent: None,
            baggage: None,
            fire_and_forget,
        };
        let dropped = |id: &str, reason: &str| {
            engine
                .observers
                .get_info(&engine, None, id)
                .unwrap()
                .dropped
                .get(reason)
                .copied()
        };

        let mut spent = test_support::request("spent", "test::src", "test::dst", None);
        spent.max_fires = Some(1);
        test_support::observe_request(&engine, &host, spent).await;
        engine
            .observers
            .get("spent")
            .unwrap()
            .fires
            .store(1, Ordering::SeqCst);
        for fire_and_forget in [true, false] {
            let refused = engine
                .observers
                .observer_call(&host, call("spent", fire_and_forget))
                .await
                .unwrap_err();
            assert_eq!(refused.code, "OBSERVER_NOT_ACTIVE");
        }
        assert_eq!(dropped("spent", "OBSERVER_NOT_ACTIVE"), Some(1));
        assert_eq!(dropped("spent", "MAX_FIRES"), Some(1));

        let mut expiring = test_support::request("expiring", "test::src", "test::dst", None);
        expiring.expires_in_ms = Some(20);
        test_support::observe_request(&engine, &host, expiring).await;
        // Disposed by the refusal, and forgotten: its counters are read
        // from here.
        let observer = engine.observers.get("expiring").unwrap();
        // Blocks the runtime's only thread: its expiry timer cannot run.
        std::thread::sleep(std::time::Duration::from_millis(30));
        let refused = engine
            .observers
            .observer_call(&host, call("expiring", true))
            .await
            .unwrap_err();
        assert_eq!(refused.code, "OBSERVER_NOT_ACTIVE");
        assert_eq!(observer.state(), crate::protocol::ObserverState::Disposed);
        assert_eq!(
            observer
                .counters()
                .drops
                .observer_not_active
                .load(Ordering::SeqCst),
            1
        );
        assert_eq!(observer.counters().drops.inactive.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn sdk_drop_reasons_are_bucketed_and_counts_saturate() {
        assert_eq!(super::sdk_drop_reason("buffer_full"), "BUFFER_FULL");
        assert_eq!(super::sdk_drop_reason("TOO_LARGE"), "TOO_LARGE");
        assert_eq!(super::sdk_drop_reason("disconnected"), "DISCONNECTED");
        assert_eq!(super::sdk_drop_reason("a-label-of-my-own"), "OTHER");
        let counter = std::sync::atomic::AtomicU64::new(u64::MAX - 1);
        super::saturating_add(&counter, 5);
        assert_eq!(counter.load(Ordering::SeqCst), u64::MAX);
    }

    /// Past the evaluator queue's capacity a completion is not evaluated and
    /// its payload is not copied: an `OVERFLOW` drop.
    #[tokio::test]
    async fn a_full_evaluator_queue_drops_with_overflow() {
        let engine = Arc::new(Engine::new());
        engine
            .observers
            .evaluator_capacity
            .store(1, Ordering::SeqCst);
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let _socket = test_support::observe(&engine, "obs", "test::src", "test::dst", None).await;
        let registry = engine.observers.clone();
        let ctx = Arc::new(InvocationCtx::engine_internal_at(0));
        let copies = registry.payload_copies();
        // No yield in between: the evaluator task cannot drain the queue.
        for n in 0..2 {
            let set = registry.watch(DEFAULT_NAMESPACE, "test::src").unwrap();
            let call = registry.begin(set, &json!({ "n": n }), &None, &None, &None);
            registry.complete(
                call,
                DEFAULT_NAMESPACE,
                "test::src",
                Uuid::new_v4(),
                &Ok(Ok(None)),
                None,
                &ctx,
            );
        }
        assert_eq!(
            registry.payload_copies(),
            copies + 1,
            "the second was not copied"
        );
        let info = registry.get_info(&engine, None, "obs").unwrap();
        assert_eq!(info.dropped.get("OVERFLOW"), Some(&1));
    }

    /// `test::src` hosted by a connection of its own, observed by `a`
    /// (whose filter no call here matches) and `b`, each with its own
    /// target. Returns the host and the targets' inputs.
    async fn hosted_source(
        engine: &Arc<Engine>,
    ) -> (
        Uuid,
        tokio::sync::mpsc::UnboundedReceiver<Value>,
        tokio::sync::mpsc::UnboundedReceiver<Value>,
        tokio::sync::mpsc::Receiver<crate::engine::Outbound>,
    ) {
        source(engine);
        let a = test_support::capture(engine, "test::a");
        let b = test_support::capture(engine, "test::b");
        let (worker, socket) = test_support::worker(engine);
        let filtered = test_support::request(
            "a",
            "test::src",
            "test::a",
            Some(json!({ "input.n": { "$eq": 99 } })),
        );
        test_support::observe_request(engine, &worker, filtered).await;
        let unfiltered = test_support::request("b", "test::src", "test::b", None);
        test_support::observe_request(engine, &worker, unfiltered).await;
        let host = Uuid::new_v4();
        engine.function_owners.insert(
            (DEFAULT_NAMESPACE.to_string(), "test::src".to_string()),
            (host, crate::engine::InvocationKind::Regular),
        );
        (host, a, b, socket)
    }

    /// Completes one observed call of `test::src` with `{ n }` as its input
    /// and output, answered by `host` with `delivery`.
    fn complete_delivered(engine: &Engine, n: u64, host: Uuid, delivery: super::RoutedDelivery) {
        let registry = &engine.observers;
        let ctx = Arc::new(InvocationCtx::engine_internal_at(0));
        let set = registry.watch(DEFAULT_NAMESPACE, "test::src").unwrap();
        let call = registry.begin(set, &json!({ "n": n }), &None, &None, &None);
        registry.complete(
            call,
            DEFAULT_NAMESPACE,
            "test::src",
            Uuid::new_v4(),
            &Ok(Ok(Some(json!({ "n": n })))),
            Some(super::HostDelivery { host, delivery }),
            &ctx,
        );
    }

    fn matched(ids: &[&str]) -> super::RoutedDelivery {
        super::RoutedDelivery::Matched(ids.iter().map(|id| id.to_string()).collect())
    }

    /// An `observe.v2` host's answer (decision 55): without `matched`, the
    /// engine activates every active observer whose filters match; with it,
    /// those of the named ones whose filters match, each once; with
    /// `delivered`, none.
    #[tokio::test]
    async fn a_hosts_answer_says_among_which_observers_the_engine_evaluates() {
        let engine = Arc::new(Engine::new());
        let (host, mut a, mut b, _socket) = hosted_source(&engine).await;
        let fires = |id: &str| engine.observers.fires(id).unwrap_or_default();
        async fn settled(engine: &Engine, before: u64) {
            until(|| engine.observers.evaluated() > before).await;
        }

        // Not narrowed: `a`'s filter takes 99 only, `b` takes everything.
        let before = engine.observers.evaluated();
        complete_delivered(&engine, 1, host, super::RoutedDelivery::Every);
        assert_eq!(test_support::next(&mut b).await, json!({ "n": 1 }));
        settled(&engine, before).await;
        assert_eq!((fires("a"), fires("b")), (0, 1));
        let before = engine.observers.evaluated();
        complete_delivered(&engine, 99, host, super::RoutedDelivery::Every);
        assert_eq!(test_support::next(&mut a).await, json!({ "n": 99 }));
        assert_eq!(test_support::next(&mut b).await, json!({ "n": 99 }));
        settled(&engine, before).await;
        assert_eq!((fires("a"), fires("b")), (1, 2));

        // Narrowed to `a` (named twice) and an id that is no observer: `a`
        // fires once when its filter matches, `b` is not named.
        let before = engine.observers.evaluated();
        complete_delivered(&engine, 99, host, matched(&["a", "a", "not-an-observer"]));
        assert_eq!(test_support::next(&mut a).await, json!({ "n": 99 }));
        settled(&engine, before).await;
        assert_eq!((fires("a"), fires("b")), (2, 2));
        let before = engine.observers.evaluated();
        complete_delivered(&engine, 1, host, matched(&["a", "b"]));
        assert_eq!(test_support::next(&mut b).await, json!({ "n": 1 }));
        settled(&engine, before).await;
        assert_eq!((fires("a"), fires("b")), (2, 3), "a's filter refuses 1");
        assert!(a.try_recv().is_err() && b.try_recv().is_err());

        // Narrowed to none, and delivered by the host: nothing is evaluated.
        let before = engine.observers.evaluated();
        complete_delivered(&engine, 99, host, matched(&[]));
        complete_delivered(&engine, 99, host, super::RoutedDelivery::Delivered);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(engine.observers.evaluated(), before);
        assert_eq!((fires("a"), fires("b")), (2, 3));

        // From anyone but the host, nothing is activated or evaluated: the
        // observers a `matched` names count `REPORTER_NOT_OWNER`, and an
        // answer that names none (no `matched`, or `delivered`) counts it
        // for every active observer.
        let not_owner = |id: &str| {
            let info = engine.observers.get_info(&engine, None, id).unwrap();
            info.dropped.get("REPORTER_NOT_OWNER").copied()
        };
        let before = engine.observers.evaluated();
        complete_delivered(&engine, 99, Uuid::new_v4(), matched(&["a"]));
        assert_eq!((not_owner("a"), not_owner("b")), (Some(1), None));
        complete_delivered(&engine, 99, Uuid::new_v4(), super::RoutedDelivery::Every);
        assert_eq!((not_owner("a"), not_owner("b")), (Some(2), Some(1)));
        complete_delivered(
            &engine,
            99,
            Uuid::new_v4(),
            super::RoutedDelivery::Delivered,
        );
        assert_eq!((not_owner("a"), not_owner("b")), (Some(3), Some(2)));
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(engine.observers.evaluated(), before);
        assert_eq!((fires("a"), fires("b")), (2, 3));
        assert!(a.try_recv().is_err() && b.try_recv().is_err());
    }

    /// When the evaluator has no room for a completion its host narrowed,
    /// only the observers its match hook named count the `OVERFLOW`.
    #[tokio::test]
    async fn a_narrowed_completion_without_room_charges_what_it_names() {
        let engine = Arc::new(Engine::new());
        engine
            .observers
            .evaluator_capacity
            .store(1, Ordering::SeqCst);
        let (host, _a, _b, _socket) = hosted_source(&engine).await;
        // No yield in between: the evaluator task cannot drain the queue.
        for n in 0..2 {
            complete_delivered(&engine, n, host, matched(&["a"]));
        }
        let dropped = |id: &str| {
            engine
                .observers
                .get_info(&engine, None, id)
                .unwrap()
                .dropped
                .get("OVERFLOW")
                .copied()
        };
        assert_eq!(dropped("a"), Some(1));
        assert_eq!(dropped("b"), None, "not listed");
    }

    /// When the evaluator has no room for a completion its host did not
    /// narrow, nothing is evaluated: every active observer counts the
    /// `OVERFLOW`, whatever its filters, as for every call the engine
    /// evaluates by itself.
    #[tokio::test]
    async fn a_completion_without_room_that_names_none_charges_every_active_observer() {
        let engine = Arc::new(Engine::new());
        engine
            .observers
            .evaluator_capacity
            .store(1, Ordering::SeqCst);
        let (host, _a, _b, _socket) = hosted_source(&engine).await;
        // No yield in between: the evaluator task cannot drain the queue.
        // `a`'s filter takes 99 only.
        for n in 0..2 {
            complete_delivered(&engine, n, host, super::RoutedDelivery::Every);
        }
        let dropped = |id: &str| {
            engine
                .observers
                .get_info(&engine, None, id)
                .unwrap()
                .dropped
                .get("OVERFLOW")
                .copied()
        };
        assert_eq!((dropped("a"), dropped("b")), (Some(1), Some(1)));
    }

    /// Completes one observed call of `test::src` with `body` as its input.
    fn complete_call(engine: &Engine, body: Value) {
        let registry = &engine.observers;
        let ctx = Arc::new(InvocationCtx::engine_internal_at(0));
        let set = registry.watch(DEFAULT_NAMESPACE, "test::src").unwrap();
        let call = registry.begin(set, &body, &None, &None, &None);
        registry.complete(
            call,
            DEFAULT_NAMESPACE,
            "test::src",
            Uuid::new_v4(),
            &Ok(Ok(None)),
            None,
            &ctx,
        );
    }

    /// A completion that does not fit what is left of the evaluator's byte
    /// budget is not queued: an `OVERFLOW` drop.
    #[tokio::test]
    async fn a_completion_past_the_byte_budget_drops_with_overflow() {
        let engine = Arc::new(Engine::new());
        let body = || json!({ "big": "x".repeat(600) });
        let size = super::approx_size(&body());
        engine
            .observers
            .evaluator_bytes
            .store(2 * size + size / 2, Ordering::SeqCst);
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let _socket = test_support::observe(&engine, "obs", "test::src", "test::dst", None).await;
        // No yield in between: the evaluator task cannot drain the queue.
        for _ in 0..3 {
            complete_call(&engine, body());
        }
        let info = engine.observers.get_info(&engine, None, "obs").unwrap();
        assert_eq!(info.dropped.get("OVERFLOW"), Some(&1), "only the third");
        assert_eq!(info.dropped.get("TOO_LARGE"), None);
    }

    /// A completion whose payloads alone are larger than the evaluator's
    /// byte budget never fits, however empty the queue: a `TOO_LARGE` drop,
    /// not `OVERFLOW`, logged once a minute with the observer, the function,
    /// the size and the limit.
    #[tokio::test]
    async fn a_completion_larger_than_the_byte_budget_is_too_large() {
        let (_logs_guard, logs) = crate::deprecation::test_support::capture();
        // Another test may have hit the warning's callsite first and cached
        // that nobody listens: recompute it now that this thread does.
        tracing::callsite::rebuild_interest_cache();
        let engine = Arc::new(Engine::new());
        engine
            .observers
            .evaluator_bytes
            .store(512, Ordering::SeqCst);
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let _socket = test_support::observe(&engine, "obs", "test::src", "test::dst", None).await;
        let big = json!({ "big": "x".repeat(1024) });
        for body in [json!({ "small": 1 }), big.clone(), big.clone()] {
            complete_call(&engine, body);
        }
        let info = engine.observers.get_info(&engine, None, "obs").unwrap();
        assert_eq!(info.dropped.get("TOO_LARGE"), Some(&2), "both big ones");
        assert_eq!(info.dropped.get("OVERFLOW"), None, "not load");

        let logs = logs.contents();
        let lines: Vec<&str> = logs.lines().filter(|l| l.contains("TOO_LARGE")).collect();
        assert_eq!(lines.len(), 1, "rate limited: {logs}");
        for expected in [
            "observer_id=obs",
            "function_id=test::src",
            "target=test::dst",
            &format!("size_bytes={}", super::approx_size(&big)),
            "limit_bytes=512",
        ] {
            assert!(lines[0].contains(expected), "{expected} in {}", lines[0]);
        }
    }

    /// An in-process target that answers only when `gate` gives it a permit,
    /// and forwards each input it got.
    fn gated(
        engine: &Engine,
        function_id: &str,
    ) -> (
        Arc<tokio::sync::Semaphore>,
        tokio::sync::mpsc::UnboundedReceiver<Value>,
    ) {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let held = gate.clone();
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: function_id.to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |input: Value| {
                let (gate, tx) = (held.clone(), tx.clone());
                async move {
                    gate.acquire().await.unwrap().forget();
                    let _ = tx.send(input);
                    FunctionResult::Success(None)
                }
            }),
        );
        (gate, rx)
    }

    async fn until(mut done: impl FnMut() -> bool) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !done() {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("in time");
    }

    fn event(n: u64) -> Arc<super::Completed> {
        Arc::new(super::Completed {
            delivery: super::Delivery::Engine,
            function: Arc::new((DEFAULT_NAMESPACE.to_string(), "test::src".to_string())),
            invocation_id: super::InvocationId::Reported(n.to_string()),
            success: true,
            output_is_input: false,
            input: Some(super::Payload::Value(Arc::new(json!({ "n": n })))),
            output: None,
            error: None,
            caller: None,
            metadata: None,
            started_at_ms: 0,
            duration_ms: 0.0,
            depth: 0,
            traceparent: None,
            baggage: None,
            queues: AtomicUsize::new(0),
            charged: AtomicUsize::new(0),
            encoded: once_cell::race::OnceBox::new(),
        })
    }

    /// The payload keeps to the call's outcome: `output` only on a success
    /// that returned something, `error` only on a failure, whatever a report
    /// carries, so a payload never has both.
    #[tokio::test]
    async fn the_payload_carries_only_what_applies_to_the_outcome() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let _target = test_support::capture(&engine, "test::dst");
        let (worker, _socket) = test_support::worker(&engine);
        let mut request = test_support::request("obs", "test::src", "test::dst", None);
        request.source.capture = Some(vec![
            crate::protocol::Capture::Output,
            crate::protocol::Capture::Error,
        ]);
        request.source.raw_input = false;
        test_support::observe_request(&engine, &worker, request).await;
        let observer = engine.observers.get("obs").expect("observer");

        let mut call = Arc::into_inner(event(1)).unwrap();
        call.error = Some(super::Payload::Value(Arc::new(
            json!({ "code": "x", "message": "stray" }),
        )));
        let payload = super::project(&observer, &call, "a");
        assert!(
            payload.get("output").is_none(),
            "returned nothing: {payload}"
        );
        assert!(payload.get("error").is_none(), "a success: {payload}");

        call.output = Some(super::Payload::Value(Arc::new(json!({ "ok": true }))));
        let payload = super::project(&observer, &call, "a");
        assert_eq!(payload["output"], json!({ "ok": true }));
        assert!(payload.get("error").is_none(), "{payload}");

        call.success = false;
        let payload = super::project(&observer, &call, "a");
        assert!(payload.get("output").is_none(), "a failure: {payload}");
        assert_eq!(payload["error"]["message"], "stray");
    }

    /// `iii.observer.activations.total` labels an activation by who chose
    /// the observers it was matched among (decision 12): `engine` when the
    /// engine evaluated every active observer, `host` when the call's host
    /// named it, on its answer (its match hook) or in a report.
    #[tokio::test]
    async fn activations_are_labelled_by_who_matched_them() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let (gate, mut delivered) = gated(&engine, "test::held");
        let (worker, _socket) = test_support::worker(&engine);
        let mut request = test_support::request("obs", "test::src", "test::held", None);
        request.max_in_flight = Some(1);
        test_support::observe_request(&engine, &worker, request).await;
        let host = Uuid::new_v4();
        engine.function_owners.insert(
            (DEFAULT_NAMESPACE.to_string(), "test::src".to_string()),
            (host, crate::engine::InvocationKind::Regular),
        );

        // The first one holds the observer's only slot: the rest queue.
        complete_call(&engine, json!({ "n": 0 }));
        until(|| engine.observers.in_flight("obs") == Some((1, 0))).await;
        complete_call(&engine, json!({ "n": 1 }));
        complete_delivered(&engine, 2, host, matched(&["obs"]));
        let full = engine.observers.report(
            host,
            DEFAULT_NAMESPACE,
            &super::RoutedAnswers::default(),
            vec![crate::protocol::ObservedItem {
                function_id: "test::src".to_string(),
                namespace: None,
                invocation_id: Uuid::new_v4().to_string(),
                matched: vec!["obs".to_string()],
                status: crate::protocol::InvocationStatus::Success,
                input: Some(crate::protocol::RawJson::from_value(&json!({ "n": 3 }))),
                output: None,
                error: None,
                caller: None,
                metadata: None,
                started_at_ms: 1,
                duration_ms: 0.5,
                observe_depth: 0,
                traceparent: None,
                baggage: None,
                output_is_input: true,
            }]
            .into(),
        );
        assert!(full.is_empty());
        until(|| engine.observers.in_flight("obs") == Some((1, 3))).await;
        // The report activates as it is read, before the evaluator gets to
        // the others: sorted, by the call each came from.
        let mut labels: Vec<(u64, &str)> = {
            let observer = engine.observers.get("obs").unwrap();
            let flow = observer.flow.write();
            flow.queue
                .iter()
                .map(|pending| {
                    let n = pending.event.input_value().unwrap()["n"].as_u64().unwrap();
                    (n, pending.event.delivery.label())
                })
                .collect()
        };
        labels.sort();
        assert_eq!(
            labels,
            [
                (1, super::ENGINE_DELIVERY),
                (2, super::HOST_DELIVERY),
                (3, super::HOST_DELIVERY)
            ]
        );

        gate.add_permits(4);
        for _ in 0..4 {
            test_support::next(&mut delivered).await;
        }
    }

    /// `iii.observer.activations.total` counts each activation under the
    /// delivery that started it (decision 12): `engine` for a call the engine
    /// evaluated against every active observer (whoever hosts it), `host`
    /// for one whose host's match hook named the observer, and
    /// `observer_call` for a call through the observer.
    #[tokio::test]
    async fn the_activation_counter_is_labelled_by_delivery() {
        let recorded = super::super::test_metrics::record();
        let engine = Arc::new(Engine::new());
        source(&engine);
        let mut delivered = test_support::capture(&engine, "test::dst");
        let (host, _socket) = test_support::worker(&engine);
        test_support::observe_request(
            &engine,
            &host,
            test_support::request("obs", "test::src", "test::dst", None),
        )
        .await;
        // The counters reach the metric when the registry exports them.
        let count = |delivery: &str| {
            engine.observers.export_drops();
            recorded.sum(
                "iii.observer.activations.total",
                &[("function_id", "test::src"), ("delivery", delivery)],
            )
        };

        complete_call(&engine, json!({ "n": 1 }));
        test_support::next(&mut delivered).await;
        assert_eq!(count(super::ENGINE_DELIVERY), 1);

        engine.function_owners.insert(
            (DEFAULT_NAMESPACE.to_string(), "test::src".to_string()),
            (host.id, crate::engine::InvocationKind::Regular),
        );
        complete_delivered(&engine, 2, host.id, matched(&["obs"]));
        test_support::next(&mut delivered).await;
        assert_eq!(count(super::HOST_DELIVERY), 1);
        complete_delivered(&engine, 3, host.id, super::RoutedDelivery::Every);
        test_support::next(&mut delivered).await;
        assert_eq!(count(super::ENGINE_DELIVERY), 2);

        engine
            .observers
            .observer_call(
                &host,
                super::ObserverCall {
                    invocation_id: Some(Uuid::new_v4()),
                    data: json!({ "n": 3 }),
                    caller: None,
                    observe_depth: None,
                    activation: crate::protocol::Activation {
                        observer_id: "obs".to_string(),
                        activation_id: None,
                    },
                    traceparent: None,
                    baggage: None,
                    fire_and_forget: false,
                },
            )
            .await
            .expect("the target answers");
        assert_eq!(test_support::next(&mut delivered).await, json!({ "n": 3 }));
        assert_eq!(
            [
                count(super::ENGINE_DELIVERY),
                count(super::HOST_DELIVERY),
                count(super::OBSERVER_CALL_DELIVERY),
            ],
            [2, 1, 1]
        );
        engine.observers.export_drops();
        assert_eq!(
            recorded.sum(
                "iii.observer.activations.total",
                &[("function_id", "test::src")]
            ),
            4,
            "no other label"
        );
    }

    /// Every observer an event matched queues the same event, not a copy of
    /// it; each target still gets its own argument, built when it runs.
    #[tokio::test]
    async fn observers_queue_one_shared_event() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let (gate, mut delivered) = gated(&engine, "test::held");
        let (worker, _socket) = test_support::worker(&engine);
        for id in ["a", "b"] {
            let mut request = test_support::request(id, "test::src", "test::held", None);
            request.max_in_flight = Some(1);
            test_support::observe_request(&engine, &worker, request).await;
        }
        for n in 1..=2 {
            engine.call("test::src", json!({ "n": n })).await.unwrap();
        }
        // Each observer runs the first event and queues the second.
        until(|| {
            ["a", "b"]
                .iter()
                .all(|id| engine.observers.in_flight(id) == Some((1, 1)))
        })
        .await;
        let queued = |id: &str| {
            let observer = engine.observers.get(id).unwrap();
            let flow = observer.flow.write();
            flow.queue[0].event.clone()
        };
        assert!(Arc::ptr_eq(&queued("a"), &queued("b")), "one event");

        gate.add_permits(4);
        let mut inputs = Vec::new();
        for _ in 0..4 {
            inputs.push(test_support::next(&mut delivered).await["n"].clone());
        }
        inputs.sort_by_key(|n| n.as_u64());
        assert_eq!(inputs, [json!(1), json!(1), json!(2), json!(2)]);
    }

    /// A queue that a burst grew gives the slots back once it drains.
    #[tokio::test]
    async fn a_drained_queue_gives_its_slots_back() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let (gate, _delivered) = gated(&engine, "test::held");
        let (worker, _socket) = test_support::worker(&engine);
        let mut request = test_support::request("obs", "test::src", "test::held", None);
        request.max_in_flight = Some(1);
        test_support::observe_request(&engine, &worker, request).await;
        for n in 0..200 {
            engine.call("test::src", json!({ "n": n })).await.unwrap();
        }
        until(|| engine.observers.in_flight("obs") == Some((1, 199))).await;
        let observer = engine.observers.get("obs").unwrap();
        assert!(observer.flow.write().queue.capacity() >= 199);

        gate.add_permits(200);
        until(|| engine.observers.in_flight("obs") == Some((0, 0))).await;
        assert!(observer.flow.write().queue.capacity() <= super::QUEUE_KEPT);
    }

    /// There is an evaluator per runtime worker thread, and a function's
    /// activations still arrive in the order its calls completed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_functions_activations_keep_their_order_across_evaluators() {
        let engine = Arc::new(Engine::new());
        for id in ["test::a", "test::b"] {
            engine.register_function_handler(
                RegisterFunctionRequest {
                    function_id: id.to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                Handler::new(|input: Value| async move { FunctionResult::Success(Some(input)) }),
            );
        }
        let (worker, _socket) = test_support::worker(&engine);
        let mut sinks = Vec::new();
        for (id, source, target) in [("oa", "test::a", "test::da"), ("ob", "test::b", "test::db")] {
            sinks.push(test_support::capture(&engine, target));
            let mut request = test_support::request(id, source, target, None);
            request.max_in_flight = Some(1);
            test_support::observe_request(&engine, &worker, request).await;
        }

        let mut callers = tokio::task::JoinSet::new();
        for caller in 0..4u64 {
            let engine = engine.clone();
            callers.spawn(async move {
                let function = if caller % 2 == 0 {
                    "test::a"
                } else {
                    "test::b"
                };
                for n in 0..50u64 {
                    let body = json!({ "caller": caller, "n": n });
                    engine.call(function, body).await.unwrap();
                }
            });
        }
        while let Some(done) = callers.join_next().await {
            done.unwrap();
        }
        assert_eq!(engine.observers.evaluators.get().map(Vec::len), Some(4));

        for mut sink in sinks {
            let mut last = std::collections::HashMap::new();
            for _ in 0..100 {
                let input = test_support::next(&mut sink).await;
                let n = input["n"].as_u64().unwrap();
                if let Some(before) = last.insert(input["caller"].as_u64().unwrap(), n) {
                    assert!(before < n, "{n} arrived after {before}");
                }
            }
        }
    }

    /// The observer `obs` of `test::src` with a queue of 4 that holds
    /// `queued` events, one more running.
    async fn full_queue(engine: &Arc<Engine>, queued: u64) -> Arc<super::Observer> {
        engine.observers.set_config(super::super::ObserversConfig {
            max_queue_per_observer: 4,
            ..Default::default()
        });
        source(engine);
        let _sink = test_support::capture(engine, "test::dst");
        let (worker, _socket) = test_support::worker(engine);
        let request = test_support::request("obs", "test::src", "test::dst", None);
        test_support::observe_request(engine, &worker, request).await;
        let observer = engine.observers.get("obs").unwrap();
        let budget = engine.observers.queue_budget.clone();
        let mut flow = observer.flow.write();
        flow.running = 1;
        for n in 0..queued {
            let event = event(n);
            assert!(budget.charge(&event));
            flow.queue
                .push_back(super::PendingActivation::queued(event, budget.clone()));
        }
        drop(flow);
        observer
    }

    /// A reader waits until the queue it overflowed is back to half its
    /// limit.
    #[tokio::test(start_paused = true)]
    async fn a_reader_waits_until_the_queue_is_half_empty() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 4).await;
        let registry = engine.observers.clone();
        let started = tokio::time::Instant::now();
        let waiting = tokio::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            async move {
                registry
                    .wait_for_room(&mut super::ReportHold::default(), &[observer])
                    .await
            }
        });
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(!waiting.is_finished(), "4 of 4 queued");
        assert!(registry.next_queued(&observer).is_some());
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(!waiting.is_finished(), "3 of 4 queued");
        assert!(registry.next_queued(&observer).is_some());
        waiting.await.unwrap();
        assert!(started.elapsed() < super::REPORT_BACKPRESSURE_WAIT);
        assert_eq!(registry.report_waits(), 1);
    }

    /// With no room coming, the reader reads on after
    /// `REPORT_BACKPRESSURE_WAIT`; at half or below it does not wait at all.
    #[tokio::test(start_paused = true)]
    async fn a_reader_waits_a_bounded_time_and_only_for_a_full_queue() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 4).await;
        let registry = engine.observers.clone();
        let started = tokio::time::Instant::now();
        registry
            .wait_for_room(
                &mut super::ReportHold::default(),
                std::slice::from_ref(&observer),
            )
            .await;
        assert!(started.elapsed() >= super::REPORT_BACKPRESSURE_WAIT);

        registry.next_queued(&observer);
        registry.next_queued(&observer);
        let started = tokio::time::Instant::now();
        registry
            .wait_for_room(&mut super::ReportHold::default(), &[observer])
            .await;
        assert_eq!(started.elapsed(), std::time::Duration::ZERO);
        assert_eq!(registry.report_waits(), 1);
    }

    /// After a hold, the reader is not held again, though the queue is still
    /// full, until it has read for a quarter as long as it waited: a worker
    /// whose reports keep overflowing is read at least a fifth of the time.
    #[tokio::test(start_paused = true)]
    async fn a_reader_is_not_held_again_until_it_read_for_a_quarter_as_long() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 4).await;
        let registry = engine.observers.clone();
        let mut hold = super::ReportHold::default();
        let wait = super::REPORT_BACKPRESSURE_WAIT;
        let read = wait / super::REPORT_HOLD_PER_READ;

        let started = tokio::time::Instant::now();
        registry
            .wait_for_room(&mut hold, std::slice::from_ref(&observer))
            .await;
        assert_eq!(started.elapsed(), wait);
        for _ in 0..3 {
            let started = tokio::time::Instant::now();
            registry
                .wait_for_room(&mut hold, std::slice::from_ref(&observer))
                .await;
            assert_eq!(started.elapsed(), std::time::Duration::ZERO);
        }
        assert_eq!(registry.report_waits(), 1);

        tokio::time::advance(read - std::time::Duration::from_millis(1)).await;
        registry
            .wait_for_room(&mut hold, std::slice::from_ref(&observer))
            .await;
        assert_eq!(registry.report_waits(), 1, "read for less than a quarter");

        tokio::time::advance(std::time::Duration::from_millis(1)).await;
        let started = tokio::time::Instant::now();
        registry.wait_for_room(&mut hold, &[observer]).await;
        assert!(started.elapsed() >= wait);
        assert_eq!(registry.report_waits(), 2);
    }

    /// A hold that ends because the queue got back to half counts too: the
    /// reader then reads for a quarter as long before it can be held again.
    #[tokio::test(start_paused = true)]
    async fn a_hold_that_ends_with_room_also_paces_the_reader() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 4).await;
        let registry = engine.observers.clone();
        let waiting = tokio::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            async move {
                let mut hold = super::ReportHold::default();
                registry.wait_for_room(&mut hold, &[observer]).await;
                hold
            }
        });
        let held = std::time::Duration::from_millis(40);
        tokio::time::sleep(held).await;
        registry.next_queued(&observer);
        registry.next_queued(&observer);
        let mut hold = waiting.await.unwrap();
        assert_eq!(registry.report_waits(), 1);

        // Full again: not held before the reader has read for 10 ms.
        let budget = registry.queue_budget.clone();
        {
            let mut flow = observer.flow.write();
            for n in 4..6 {
                let event = event(n);
                assert!(budget.charge(&event));
                flow.queue
                    .push_back(super::PendingActivation::queued(event, budget.clone()));
            }
        }
        let read = held / super::REPORT_HOLD_PER_READ;
        tokio::time::advance(read - std::time::Duration::from_millis(1)).await;
        registry
            .wait_for_room(&mut hold, std::slice::from_ref(&observer))
            .await;
        assert_eq!(registry.report_waits(), 1);
        tokio::time::advance(std::time::Duration::from_millis(1)).await;
        registry.wait_for_room(&mut hold, &[observer]).await;
        assert_eq!(registry.report_waits(), 2);
    }

    /// A disposed observer's queue is gone: the reader reads on at once.
    #[tokio::test(start_paused = true)]
    async fn a_reader_waiting_on_a_disposed_observer_reads_on() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 4).await;
        let registry = engine.observers.clone();
        let started = tokio::time::Instant::now();
        let waiting = tokio::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            async move {
                registry
                    .wait_for_room(&mut super::ReportHold::default(), &[observer])
                    .await
            }
        });
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        registry.dispose(
            &observer,
            crate::protocol::observe_codes::UNOBSERVED,
            "test",
        );
        waiting.await.unwrap();
        assert!(started.elapsed() < super::REPORT_BACKPRESSURE_WAIT);
        assert_eq!(
            observer.drop_snapshot().get("INACTIVE"),
            Some(&4),
            "the queue's events"
        );
    }

    /// An event waiting in several queues counts once against the queues'
    /// budget, and gives its share back when the last queue lets it go.
    #[tokio::test]
    async fn a_queued_event_counts_once_against_the_budget() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let (gate, _delivered) = gated(&engine, "test::held");
        let (worker, _socket) = test_support::worker(&engine);
        for id in ["a", "b"] {
            let mut request = test_support::request(id, "test::src", "test::held", None);
            request.max_in_flight = Some(1);
            test_support::observe_request(&engine, &worker, request).await;
        }
        for n in 1..=2 {
            engine.call("test::src", json!({ "n": n })).await.unwrap();
        }
        until(|| {
            ["a", "b"]
                .iter()
                .all(|id| engine.observers.in_flight(id) == Some((1, 1)))
        })
        .await;
        let budget = &engine.observers.queue_budget;
        let queued = {
            let observer = engine.observers.get("a").unwrap();
            let flow = observer.flow.write();
            flow.queue[0].event.clone()
        };
        assert_eq!(budget.bytes.load(Ordering::SeqCst), queued.cost());

        gate.add_permits(4);
        until(|| {
            ["a", "b"]
                .iter()
                .all(|id| engine.observers.in_flight(id) == Some((0, 0)))
        })
        .await;
        assert_eq!(budget.bytes.load(Ordering::SeqCst), 0);
    }

    /// Past the queues' budget an activation that would queue is dropped as
    /// `OVERFLOW`, though its own queue has room.
    #[tokio::test]
    async fn past_the_queues_budget_activations_overflow() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let (gate, _delivered) = gated(&engine, "test::held");
        let (worker, _socket) = test_support::worker(&engine);
        let mut request = test_support::request("obs", "test::src", "test::held", None);
        request.max_in_flight = Some(1);
        test_support::observe_request(&engine, &worker, request).await;
        // The first runs, the second queues: the budget is then made room
        // for two events like it.
        for n in 0..2 {
            engine.call("test::src", json!({ "n": n })).await.unwrap();
        }
        until(|| engine.observers.in_flight("obs") == Some((1, 1))).await;
        let cost = {
            let observer = engine.observers.get("obs").unwrap();
            let flow = observer.flow.write();
            flow.queue[0].event.cost()
        };
        let limit = 2 * cost + cost / 2;
        engine
            .observers
            .queue_budget
            .limit
            .store(limit, Ordering::SeqCst);
        for n in 2..6 {
            engine.call("test::src", json!({ "n": n })).await.unwrap();
        }
        until(|| engine.observers.evaluated() >= 6).await;
        assert_eq!(engine.observers.in_flight("obs"), Some((1, 2)));
        let info = engine.observers.get_info(&engine, None, "obs").unwrap();
        assert_eq!(info.dropped.get("OVERFLOW"), Some(&3));
        assert!(engine.observers.queue_budget.bytes.load(Ordering::SeqCst) <= limit);
        gate.add_permits(3);
        until(|| engine.observers.in_flight("obs") == Some((0, 0))).await;
        assert_eq!(
            engine.observers.queue_budget.bytes.load(Ordering::SeqCst),
            0
        );
    }

    /// A full queue's drop counts no fire, with or without `max_fires`; the
    /// fire that reaches `max_fires` disposes the observer, and what comes
    /// after it drops as `INACTIVE`.
    #[tokio::test]
    async fn an_overflow_counts_no_fire() {
        for max_fires in [None, Some(10), Some(2)] {
            let engine = Arc::new(Engine::new());
            engine.observers.set_config(super::super::ObserversConfig {
                max_queue_per_observer: 1,
                ..Default::default()
            });
            source(&engine);
            let (_gate, _delivered) = gated(&engine, "test::held");
            let (worker, _socket) = test_support::worker(&engine);
            let mut request = test_support::request("obs", "test::src", "test::held", None);
            request.max_in_flight = Some(1);
            request.max_fires = max_fires;
            test_support::observe_request(&engine, &worker, request).await;
            let observer = engine.observers.get("obs").unwrap();
            let registry = engine.observers.clone();
            // One runs (held by the gate), one queues, the third finds the
            // queue full, or the observer spent.
            let overflowed: Vec<bool> = (0..3)
                .map(|n| registry.activate(&observer, &event(n)))
                .collect();
            let spent = max_fires == Some(2);
            assert_eq!(
                overflowed,
                [false, false, !spent],
                "max_fires {max_fires:?}"
            );
            assert_eq!(observer.fires.load(Ordering::SeqCst), 2, "{max_fires:?}");
            let drops = observer.drop_snapshot();
            let (reason, other) = if spent {
                ("INACTIVE", "OVERFLOW")
            } else {
                ("OVERFLOW", "INACTIVE")
            };
            assert_eq!(drops.get(reason), Some(&1), "{max_fires:?}: {drops:?}");
            assert_eq!(drops.get(other), None, "{max_fires:?}: {drops:?}");
            assert_eq!(drops.get("MAX_FIRES"), None, "{max_fires:?}: {drops:?}");
        }
    }

    /// An activation that passed the state check and then waits for the
    /// observer's flow lock while the observer is disposed must not run: it is
    /// dropped `INACTIVE`, with no fire counted. The interleaving is
    /// forced with the lock held, by sleeps (a late thread only makes the
    /// test pass early, never fail).
    #[tokio::test]
    async fn an_activation_that_loses_a_race_with_dispose_does_not_fire() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let (worker, _socket) = test_support::worker(&engine);
        let request = test_support::request("obs", "test::src", "test::dst", None);
        test_support::observe_request(&engine, &worker, request).await;
        let observer = engine.observers.get("obs").unwrap();
        let registry = engine.observers.clone();

        let held = observer.flow.write();
        let activate = std::thread::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            move || registry.activate(&observer, &event(0))
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        let dispose = std::thread::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            move || registry.dispose(&observer, "UNOBSERVED", "disposed")
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        drop(held);
        activate.join().unwrap();
        dispose.join().unwrap();

        assert_eq!(observer.fires.load(Ordering::SeqCst), 0, "no fire counted");
        assert_eq!(observer.drop_snapshot().get("INACTIVE"), Some(&1));
    }

    /// `max_fires` 2: an activation took fire 1 and waits for the flow lock
    /// while fire 2 is taken and the observer disposed `MAX_FIRES`, as the
    /// activation that takes the last fire does. Fire 1 is within
    /// `max_fires`: it runs, and its fire is kept.
    #[tokio::test]
    async fn a_fire_the_last_fire_overtook_still_runs() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let mut sink = test_support::capture(&engine, "test::dst");
        let (worker, _socket) = test_support::worker(&engine);
        let mut request = test_support::request("mf", "test::src", "test::dst", None);
        request.max_fires = Some(2);
        test_support::observe_request(&engine, &worker, request).await;
        let observer = engine.observers.get("mf").unwrap();
        let registry = engine.observers.clone();

        let held = observer.flow.write();
        let first = std::thread::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            move || registry.activate(&observer, &event(0))
        });
        // Until the first activation holds its fire: it is past the state
        // check and waits for the lock.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while observer.fires.load(Ordering::SeqCst) == 0 {
            assert!(std::time::Instant::now() < deadline, "fire 1 taken");
            std::thread::yield_now();
        }
        assert_eq!(super::try_fire(&observer), Some(2));
        let last = std::thread::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            move || {
                registry.dispose(
                    &observer,
                    observe_codes::MAX_FIRES,
                    "the observer fired 2 times, its max_fires",
                )
            }
        });
        last.join().unwrap();
        drop(held);
        first.join().unwrap();

        assert_eq!(observer.fires.load(Ordering::SeqCst), 2);
        assert_eq!(observer.drop_snapshot().get("INACTIVE"), None);
        let delivered = tokio::time::timeout(std::time::Duration::from_secs(5), sink.recv()).await;
        assert!(
            matches!(delivered, Ok(Some(_))),
            "fire 1 reached the target"
        );
    }

    /// A call through an observer that passed `check_observer_call` and waits
    /// for the flow lock while the observer is disposed is refused
    /// `OBSERVER_NOT_ACTIVE`, its fire given back, as the check refuses it.
    #[tokio::test]
    async fn a_call_through_an_observer_that_loses_a_race_with_dispose_is_refused() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let (worker, _socket) = test_support::worker(&engine);
        let request = test_support::request("oc", "test::src", "test::dst", None);
        test_support::observe_request(&engine, &worker, request).await;
        let observer = engine.observers.get("oc").unwrap();
        let registry = engine.observers.clone();
        assert!(observer.is_active());

        let held = observer.flow.write();
        let admit = std::thread::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            move || registry.admit_observer_call(&observer, 0, true).map(drop)
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while observer.fires.load(Ordering::SeqCst) == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the call took its fire"
            );
            std::thread::yield_now();
        }
        // Disposal sets the state before it waits for the flow lock.
        let dispose = std::thread::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            move || registry.dispose(&observer, "UNOBSERVED", "disposed")
        });
        while observer.state() != ObserverState::Disposed {
            assert!(std::time::Instant::now() < deadline, "disposed");
            std::thread::yield_now();
        }
        drop(held);
        let refused = admit.join().unwrap().expect_err("refused");
        dispose.join().unwrap();

        assert_eq!(refused.code, observe_codes::OBSERVER_NOT_ACTIVE);
        assert_eq!(observer.fires.load(Ordering::SeqCst), 0, "fire given back");
        assert_eq!(
            observer
                .drop_snapshot()
                .get(observe_codes::OBSERVER_NOT_ACTIVE),
            Some(&1),
            "a fire-and-forget refusal counts under its code"
        );
        assert_eq!(observer.flow.write().running, 0);
    }

    /// A call through an observer is the observing worker's call (decision
    /// 57): a target private to the observing worker takes it from the
    /// host, and a direct call of the target from any other worker is
    /// refused. The target turns private as a registration makes it, before
    /// that registration reaches the observer.
    #[tokio::test]
    async fn a_call_through_an_observer_is_the_observing_workers_call_of_a_private_target() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let mut delivered = test_support::capture(&engine, "test::dst");
        let (observing, _observing_socket) = test_support::worker(&engine);
        let (host, _host_socket) = test_support::worker(&engine);
        let (third, _third_socket) = test_support::worker(&engine);
        test_support::observe_request(
            &engine,
            &observing,
            test_support::request("obs", "test::src", "test::dst", None),
        )
        .await;
        for (function_id, connection) in [("test::src", host.id), ("test::dst", observing.id)] {
            engine.function_owners.insert(
                (DEFAULT_NAMESPACE.to_string(), function_id.to_string()),
                (connection, crate::engine::InvocationKind::Regular),
            );
        }
        engine.observers.set_options(
            DEFAULT_NAMESPACE,
            "test::dst",
            super::super::FunctionOptions::private(),
        );
        assert!(
            engine
                .observers
                .hosts_privately(DEFAULT_NAMESPACE, "test::dst", observing.id)
        );
        assert!(engine.observers.get("obs").unwrap().is_active());

        let answer = engine
            .observers
            .observer_call(
                &host,
                super::ObserverCall {
                    invocation_id: Some(Uuid::new_v4()),
                    data: json!({ "n": 1 }),
                    caller: None,
                    observe_depth: None,
                    activation: crate::protocol::Activation {
                        observer_id: "obs".to_string(),
                        activation_id: None,
                    },
                    traceparent: None,
                    baggage: None,
                    fire_and_forget: false,
                },
            )
            .await;
        assert!(answer.is_ok(), "{answer:?}");
        assert_eq!(test_support::next(&mut delivered).await, json!({ "n": 1 }));

        let handler = engine
            .functions
            .get_handler(DEFAULT_NAMESPACE, "test::dst")
            .unwrap();
        for caller in [&third, &host] {
            let direct = engine
                .invocations
                .handle_invocation_with_handler(
                    Some(Uuid::new_v4()),
                    Some(caller),
                    "test::dst".to_string(),
                    DEFAULT_NAMESPACE,
                    json!({ "n": 2 }),
                    handler.clone(),
                    None,
                    None,
                    None,
                    None,
                    InvocationCtx::engine_internal(),
                )
                .await
                .expect("answered");
            assert_eq!(direct.unwrap_err().code, observe_codes::FORBIDDEN);
        }
        assert!(delivered.try_recv().is_err(), "no direct call ran");
    }

    /// An event larger than the whole of the queues' budget never queues:
    /// while the observer is busy it is a `TOO_LARGE` drop, not `OVERFLOW`,
    /// and a report's reader does not wait for room that cannot come. Events
    /// that fit still queue, then overflow as the budget fills.
    #[tokio::test]
    async fn an_event_larger_than_the_queues_budget_is_too_large() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 1).await;
        let registry = engine.observers.clone();
        let cost = event(0).cost();
        registry
            .queue_budget
            .limit
            .store(3 * cost, Ordering::SeqCst);
        let mut big = Arc::into_inner(event(9)).unwrap();
        big.input = Some(super::Payload::Value(Arc::new(
            json!({ "big": "x".repeat(4 * cost) }),
        )));
        let big = Arc::new(big);
        assert!(!registry.activate(&observer, &big), "no room to wait for");
        for n in 1..=2 {
            assert!(!registry.activate(&observer, &event(n)), "queued");
        }
        assert!(
            registry.activate(&observer, &event(3)),
            "the budget is full"
        );
        let dropped = observer.drop_snapshot();
        assert_eq!(dropped.get("TOO_LARGE"), Some(&1));
        assert_eq!(dropped.get("OVERFLOW"), Some(&1));
        assert_eq!(registry.in_flight("obs"), Some((1, 3)));
    }

    /// A reader also waits while the queues together are over half their
    /// budget, though the queue it overflowed is not.
    #[tokio::test(start_paused = true)]
    async fn a_reader_waits_for_the_queues_budget() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 1).await;
        let registry = engine.observers.clone();
        let budget = registry.queue_budget.clone();
        // The one queued event is over half of this budget.
        budget.limit.store(event(0).cost() + 1, Ordering::SeqCst);
        let waiting = tokio::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            async move {
                registry
                    .wait_for_room(&mut super::ReportHold::default(), &[observer])
                    .await
            }
        });
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(!waiting.is_finished(), "the budget is more than half full");
        drop(registry.next_queued(&observer));
        waiting.await.unwrap();
        assert_eq!(budget.bytes.load(Ordering::SeqCst), 0);
    }

    /// For a target a worker connection hosts, the argument is the JSON text
    /// of the value `project` builds, whatever the observer delivers and
    /// captures, and the event's payloads are encoded once.
    #[test]
    fn an_encoded_projection_is_the_projection() {
        use super::super::lifecycle::{
            CaptureSet, Deliver, Flags, NewObserver, Observer, Observing, Policy,
        };

        fn observer(deliver: Deliver, capture: CaptureSet) -> Observer {
            Observer::new(NewObserver {
                id: "o-1".to_string(),
                function: Arc::new((DEFAULT_NAMESPACE.to_string(), "test::src".to_string())),
                target: Arc::new((DEFAULT_NAMESPACE.to_string(), "test::sink".to_string())),
                spec: None,
                args: json!({}),
                filter: iii_filter::Filter::match_all(),
                capture,
                deliver,
                metadata: None,
                policy: Policy {
                    max_fires: None,
                    expires_in_ms: None,
                    expires_at: None,
                    max_in_flight: 1,
                    max_queue: 1,
                },
                warnings: Vec::new(),
                observing: Observing {
                    connection: Uuid::nil(),
                    worker_name: None,
                    namespace: Arc::from(DEFAULT_NAMESPACE),
                    session: None,
                    answered: true,
                },
                flags: Flags {
                    observing_connected: true,
                    function_available: true,
                    target_available: true,
                    owner_acked: true,
                    returning: false,
                    announced: None,
                    rejected_by_owner: false,
                    activated_once: false,
                    failed: None,
                    disposed: None,
                    tombstone: false,
                },
            })
        }

        fn completed(
            success: bool,
            parts: [Option<Arc<Value>>; 5],
            duration_ms: f64,
        ) -> super::Completed {
            let [input, output, error, caller, metadata] = parts;
            super::Completed {
                delivery: super::Delivery::Engine,
                function: Arc::new((DEFAULT_NAMESPACE.to_string(), "test::src".to_string())),
                invocation_id: super::InvocationId::Reported("i-\"1\"".to_string()),
                success,
                output_is_input: false,
                input: input.map(super::Payload::Value),
                output: output.map(super::Payload::Value),
                error: error.map(super::Payload::Value),
                caller,
                metadata: metadata.map(super::Payload::Value),
                started_at_ms: 1_700_000_000_123,
                duration_ms,
                depth: 0,
                traceparent: None,
                baggage: None,
                queues: AtomicUsize::new(0),
                charged: AtomicUsize::new(0),
                encoded: once_cell::race::OnceBox::new(),
            }
        }

        let large = |n: u64| json!({ "n": n, "big": "x".repeat(4096), "_caller_worker_id": "w" });
        let event = Arc::new(json!({ "k": [1, "two \u{e9}", { "x": null, "y": 2.5 }] }));
        let caller = Arc::new(json!({ "worker_name": "iii-engine" }));
        let reported = |text: &str| Some(super::Payload::text(Arc::from(text)));
        let events = [
            // An event source's: its input and output are one value.
            completed(
                true,
                [
                    Some(event.clone()),
                    Some(event.clone()),
                    None,
                    Some(caller.clone()),
                    None,
                ],
                0.25,
            ),
            // A failed call's, with every part kept.
            completed(
                false,
                [
                    Some(Arc::new(json!({ "order": 7 }))),
                    None,
                    Some(Arc::new(json!({ "code": "E", "message": "m" }))),
                    Some(caller.clone()),
                    Some(Arc::new(json!({ "tenant": "t" }))),
                ],
                f64::NAN,
            ),
            // Nothing kept.
            completed(true, [None, None, None, None, None], 3.0),
            // A call's that kept its large input and output as text.
            {
                let mut kept = completed(true, [None, None, None, None, None], 1.5);
                kept.input = Some(super::Payload::of(&large(1)));
                kept.output = Some(super::Payload::of(&large(2)));
                kept
            },
            // A report's: its payloads the text its host sent, its output
            // its input.
            {
                let mut kept = completed(true, [None, None, None, Some(caller), None], 0.5);
                kept.output_is_input = true;
                kept.input = reported(r#"{"z":[1.50,null],"a":"\u00e9"}"#);
                kept.error = reported(r#"{"code": "E"}"#);
                kept.metadata = reported(r#"{"tenant":"t"}"#);
                kept
            },
        ];
        let every = CaptureSet {
            input: true,
            output: true,
            error: true,
            caller: true,
            metadata: true,
            on_success: true,
            on_error: true,
        };
        let none = CaptureSet {
            input: false,
            output: false,
            error: false,
            caller: false,
            metadata: false,
            on_success: true,
            on_error: true,
        };
        for (n, completed) in events.iter().enumerate() {
            for deliver in [Deliver::Payload, Deliver::Input, Deliver::Output] {
                for capture in [CaptureSet::DEFAULT, every, none] {
                    let observer = observer(deliver, capture);
                    let mut text = String::new();
                    let encoded = super::project_encoded(&observer, completed, "a-1");
                    encoded.write_to(&mut text);
                    assert_eq!(text.len(), encoded.len());
                    assert_eq!(
                        serde_json::from_str::<Value>(&text).unwrap(),
                        super::project(&observer, completed, "a-1"),
                        "event {n}, {deliver:?}, {capture:?}"
                    );
                }
            }
        }
        // The event source's value was encoded once, as its input.
        let text = events[0]
            .encoded
            .get()
            .expect("encoded for a worker's target");
        assert!(text.input.get().is_some_and(Option::is_some));
        assert!(text.output.get().is_none());
        // A payload kept as text is sent as it is, and decoded once for the
        // projections that take a value.
        for payload in [&events[3].input, &events[3].output] {
            let Some(super::Payload::Text(text)) = payload else {
                panic!("kept as text");
            };
            assert!(text.value.get().is_some_and(Option::is_some));
        }
        assert!(events[3].encoded.get().is_none(), "nothing to encode");
        // A report's text is sent as it is (only its caller is encoded),
        // and its input decoded once, for its output too.
        let text = events[4].encoded.get().expect("the caller encoded");
        assert!(text.caller.get().is_some_and(Option::is_some));
        for part in [&text.input, &text.output, &text.error, &text.metadata] {
            assert!(part.get().is_none());
        }
        assert!(std::ptr::eq(
            events[4].input_value().unwrap(),
            events[4].output_value().unwrap()
        ));
    }

    /// An event holds each of its payloads as a `Payload` (a tag and a
    /// pointer) in no more room than it took with each a pointer: its
    /// delivery label is one byte, its trace context boxed and its encoded
    /// parts behind one pointer.
    #[cfg(target_pointer_width = "64")]
    #[test]
    fn an_event_holds_its_payloads_in_no_more_room() {
        let size = std::mem::size_of::<super::Completed>();
        assert!(size <= 184, "{size} bytes");
    }

    /// A charge refused for lack of room leaves the event's cost behind: the
    /// next observer's charge reads it rather than walking the payloads.
    #[test]
    fn a_refused_charge_keeps_the_events_cost() {
        let budget = super::QueueBudget::default();
        budget.limit.store(1, Ordering::SeqCst);
        let event = event(0);
        assert!(!budget.charge(&event));
        let cost = event.charged.load(Ordering::SeqCst);
        assert_eq!(cost, event.cost());
        assert!(!budget.charge(&event));
        assert_eq!(event.charged.load(Ordering::SeqCst), cost);
        assert_eq!(event.queues.load(Ordering::SeqCst), 0);
        assert_eq!(budget.too_large(&event), Some((cost, 1)));
    }

    /// A call keeps of its input what its observers read: the fields their
    /// filters read when that is all they read, its JSON text when one of
    /// them captures or delivers it; never the engine's caller stamp. The
    /// filters still match as they would on the whole input.
    #[tokio::test]
    async fn a_call_keeps_what_its_observers_read_of_the_input() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let mut delivered = test_support::capture(&engine, "test::dst");
        let (worker, _socket) = test_support::worker(&engine);
        let mut filtered = test_support::request(
            "filtered",
            "test::src",
            "test::dst",
            Some(json!({ "input.k": { "$eq": 1 } })),
        );
        filtered.source.capture = Some(vec![crate::protocol::Capture::Output]);
        filtered.source.raw_input = false;
        test_support::observe_request(&engine, &worker, filtered).await;
        let body = |k: u64| json!({ "k": k, "big": [1, 2, 3], "_caller_worker_id": "w-1" });
        let keep = |body: &Value| {
            let set = engine
                .observers
                .watch(DEFAULT_NAMESPACE, "test::src")
                .unwrap();
            let mut call = engine.observers.begin(set, body, &None, &None, &None);
            let kept = call.inner.take().unwrap();
            match kept.input {
                Some(super::Payload::Value(value)) => (Some(Value::clone(&value)), None),
                Some(super::Payload::Text(kept)) => (None, Some(kept.text)),
                None => (None, None),
            }
        };
        assert_eq!(keep(&body(1)), (Some(json!({ "k": 1 })), None));

        // The filter reads `input.k` of the fields kept.
        let ctx = Arc::new(InvocationCtx::engine_internal_at(0));
        for k in [2, 1] {
            let set = engine
                .observers
                .watch(DEFAULT_NAMESPACE, "test::src")
                .unwrap();
            let call = engine.observers.begin(set, &body(k), &None, &None, &None);
            engine.observers.complete(
                call,
                DEFAULT_NAMESPACE,
                "test::src",
                Uuid::new_v4(),
                &Ok(Ok(Some(json!({ "seen": k })))),
                None,
                &ctx,
            );
        }
        let envelope = test_support::next(&mut delivered).await;
        assert_eq!(envelope["output"], json!({ "seen": 1 }));
        assert!(envelope.get("input").is_none());

        // An observer that captures the input: all of it, a copy when it is
        // small, its text when it is large.
        test_support::observe_request(
            &engine,
            &worker,
            test_support::request("whole", "test::src", "test::dst", None),
        )
        .await;
        assert_eq!(
            keep(&body(1)),
            (Some(json!({ "k": 1, "big": [1, 2, 3] })), None)
        );
        let large = json!({ "k": 1, "big": "x".repeat(4096), "_caller_worker_id": "w-1" });
        let (input, text) = keep(&large);
        assert_eq!(input, None);
        assert_eq!(
            serde_json::from_str::<Value>(&text.unwrap()).unwrap(),
            json!({ "k": 1, "big": "x".repeat(4096) })
        );
    }

    fn fresh_ids(n: usize) -> Vec<Uuid> {
        (0..n).map(|_| Uuid::new_v4()).collect()
    }

    /// Past its window, `RoutedAnswers` forgets the ids it kept first, one
    /// per new id, in the order it kept them.
    #[test]
    fn routed_answers_forget_the_oldest_first() {
        let answered = RoutedAnswers::default();
        let ids = fresh_ids(ROUTED_ANSWERS_KEPT + 100);
        for (n, id) in ids.iter().enumerate() {
            answered.record(*id);
            if let Some(forgotten) = n.checked_sub(ROUTED_ANSWERS_KEPT) {
                assert!(!answered.contains(&ids[forgotten]));
                assert!(answered.contains(&ids[forgotten + 1]));
            }
        }
        assert!(ids[..100].iter().all(|id| !answered.contains(id)));
        assert!(ids[100..].iter().all(|id| answered.contains(id)));
    }

    /// An id is kept from its answer until `ROUTED_ANSWERS_KEPT` other ids
    /// were answered after it, and again once it is answered again.
    #[test]
    fn routed_answers_keep_an_id_until_the_window_passes_it() {
        let answered = RoutedAnswers::default();
        let id = Uuid::new_v4();
        assert!(!answered.contains(&id));
        answered.record(id);
        assert!(answered.contains(&id));
        for other in fresh_ids(ROUTED_ANSWERS_KEPT - 1) {
            answered.record(other);
        }
        assert!(answered.contains(&id), "still in the window");
        answered.record(Uuid::new_v4());
        assert!(!answered.contains(&id), "forgotten past the window");
        answered.record(id);
        assert!(answered.contains(&id), "kept again");
    }

    /// A repeated id takes no second place and keeps its first: it pushes no
    /// other id out, and it is forgotten when its first answer is the oldest.
    #[test]
    fn routed_answers_keep_a_repeated_id_once() {
        let answered = RoutedAnswers::default();
        let ids = fresh_ids(ROUTED_ANSWERS_KEPT);
        for id in &ids {
            answered.record(*id);
        }
        answered.record(ids[0]);
        answered.record(ids[ROUTED_ANSWERS_KEPT - 1]);
        assert!(ids.iter().all(|id| answered.contains(id)));
        let newest = Uuid::new_v4();
        answered.record(newest);
        assert!(!answered.contains(&ids[0]));
        assert!(ids[1..].iter().all(|id| answered.contains(id)));
        assert!(answered.contains(&newest));
    }

    /// `RoutedAnswers` keeps the ids a set of the kept ids with a queue of
    /// them in the order they were kept keeps (what it was before), through
    /// repeats, evictions and ids answered again after their eviction.
    #[test]
    fn routed_answers_keep_what_a_set_and_a_queue_keep() {
        use rand::{Rng, SeedableRng};

        let mut rng = rand::rngs::StdRng::seed_from_u64(38);
        let pool = fresh_ids(ROUTED_ANSWERS_KEPT + ROUTED_ANSWERS_KEPT / 2);
        let answered = RoutedAnswers::default();
        let mut set = std::collections::HashSet::new();
        let mut order = std::collections::VecDeque::new();
        for step in 1..=8 * ROUTED_ANSWERS_KEPT {
            let id = pool[rng.gen_range(0..pool.len())];
            answered.record(id);
            if set.insert(id) {
                order.push_back(id);
                if order.len() > ROUTED_ANSWERS_KEPT {
                    set.remove(&order.pop_front().unwrap());
                }
            }
            if step % 1024 == 0 {
                for id in &pool {
                    assert_eq!(answered.contains(id), set.contains(id), "step {step}");
                }
            }
        }
        assert!(!answered.contains(&Uuid::new_v4()));
    }

    /// The window takes no room before the first answer, and never more than
    /// `ROUTED_ANSWERS_KEPT` ids and twice as many index slots.
    #[test]
    fn routed_answers_never_grow_past_the_window() {
        let room = |answered: &RoutedAnswers| {
            let kept = super::lock(&answered.kept);
            let ids = kept.ids.capacity().max(kept.hashes.capacity());
            (ids, kept.index.len())
        };
        let answered = RoutedAnswers::default();
        assert_eq!(room(&answered), (0, 0));
        for id in fresh_ids(100) {
            answered.record(id);
        }
        let (ids, slots) = room(&answered);
        assert!(ids <= 128 && slots <= 256, "{ids} ids, {slots} slots");
        for id in fresh_ids(10 * ROUTED_ANSWERS_KEPT) {
            answered.record(id);
            let (ids, slots) = room(&answered);
            assert!(ids <= ROUTED_ANSWERS_KEPT && slots <= 2 * ROUTED_ANSWERS_KEPT);
        }
    }

    /// An event's invocation id is written as the engine wrote it, or as the
    /// report gave it: a reported id is kept as a `Uuid` only when that
    /// writes back the same text.
    #[test]
    fn an_invocation_id_is_written_as_it_was_given() {
        use super::InvocationId;
        let written =
            |id: &InvocationId| (id.to_string(), serde_json::to_string(id).unwrap(), id.len());
        let uuid = Uuid::new_v4();
        let engine = InvocationId::Uuid(uuid);
        assert_eq!(
            written(&engine),
            (uuid.to_string(), format!("\"{uuid}\""), 36)
        );
        assert_eq!(InvocationId::reported(uuid.to_string(), Some(uuid)), engine);
        for given in [
            uuid.to_string().to_uppercase(),
            uuid.simple().to_string(),
            uuid.urn().to_string(),
        ] {
            let parsed = Uuid::parse_str(&given).ok();
            assert_eq!(parsed, Some(uuid), "{given}");
            let id = InvocationId::reported(given.clone(), parsed);
            assert_eq!(id, InvocationId::Reported(given.clone()));
            assert_eq!(
                written(&id),
                (given.clone(), format!("\"{given}\""), given.len())
            );
        }
        let odd = InvocationId::reported("i-\"1\"".to_string(), None);
        assert_eq!(serde_json::to_string(&odd).unwrap(), r#""i-\"1\"""#);
        assert_eq!(odd.len(), 5);
    }

    /// An observer of `test::src` (or `source`) delivering the envelope to
    /// the gated `test::held`, one activation at a time.
    async fn one_at_a_time(
        engine: &Arc<Engine>,
        source: &str,
    ) -> (
        Arc<tokio::sync::Semaphore>,
        tokio::sync::mpsc::UnboundedReceiver<Value>,
        tokio::sync::mpsc::Receiver<crate::engine::Outbound>,
    ) {
        let (gate, delivered) = gated(engine, "test::held");
        let (worker, socket) = test_support::worker(engine);
        let mut request = test_support::request("obs", source, "test::held", None);
        // The envelope (the merged equivalent of #2480's `deliver: envelope`).
        request.source.raw_input = false;
        request.max_in_flight = Some(1);
        test_support::observe_request(engine, &worker, request).await;
        (gate, delivered, socket)
    }

    /// A routed call's event shares the key its function's active set holds,
    /// keeps the engine's invocation id as a `Uuid` and holds no JSON text
    /// while only in-process targets read it; the envelope names the
    /// function and the call as before.
    #[tokio::test]
    async fn an_event_shares_its_functions_key_and_encodes_nothing_for_in_process_targets() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let (gate, mut delivered, _socket) = one_at_a_time(&engine, "test::src").await;
        let registry = &engine.observers;
        let set = registry
            .watch(DEFAULT_NAMESPACE, "test::src")
            .expect("observed");
        let ctx = Arc::new(InvocationCtx::engine_internal_at(0));
        let ids = [Uuid::new_v4(), Uuid::new_v4()];
        for id in ids {
            let call = registry.begin(set.clone(), &json!({ "n": 1 }), &None, &None, &None);
            registry.complete(
                call,
                DEFAULT_NAMESPACE,
                "test::src",
                id,
                &Ok(Ok(None)),
                None,
                &ctx,
            );
        }
        // The first holds the observer's only slot: the second queues.
        until(|| registry.in_flight("obs") == Some((1, 1))).await;
        {
            let observer = registry.get("obs").unwrap();
            let flow = observer.flow.write();
            let event = &flow.queue[0].event;
            assert!(Arc::ptr_eq(&event.function, &set.function));
            assert_eq!(event.invocation_id, super::InvocationId::Uuid(ids[1]));
            assert!(
                event.encoded.get().is_none(),
                "no text for in-process targets"
            );
        }
        gate.add_permits(2);
        for id in ids {
            let envelope = test_support::next(&mut delivered).await;
            assert_eq!(envelope["invocation_id"], id.to_string());
            assert_eq!(envelope["function_id"], "test::src");
            assert_eq!(envelope["namespace"], DEFAULT_NAMESPACE);
        }
    }

    /// A reported event shares its function's key with the active set too,
    /// keeps a reported id written as the engine writes one as a `Uuid`, and
    /// delivers every id as the report gave it.
    #[tokio::test]
    async fn a_reported_event_shares_the_key_and_keeps_the_id_it_was_given() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let (gate, mut delivered, _socket) = one_at_a_time(&engine, "test::src").await;
        let host = Uuid::new_v4();
        engine.function_owners.insert(
            (DEFAULT_NAMESPACE.to_string(), "test::src".to_string()),
            (host, crate::engine::InvocationKind::Regular),
        );
        let set = engine
            .observers
            .watch(DEFAULT_NAMESPACE, "test::src")
            .expect("observed");
        let uuid = Uuid::new_v4();
        let given = [
            "local-1".to_string(),
            uuid.to_string(),
            uuid.to_string().to_uppercase(),
        ];
        let item = |invocation_id: &String| crate::protocol::ObservedItem {
            function_id: "test::src".to_string(),
            namespace: None,
            invocation_id: invocation_id.clone(),
            matched: vec!["obs".to_string()],
            status: crate::protocol::InvocationStatus::Success,
            input: Some(crate::protocol::RawJson::from_value(&json!({ "n": 1 }))),
            output: None,
            error: None,
            caller: None,
            metadata: None,
            started_at_ms: 1,
            duration_ms: 0.5,
            observe_depth: 0,
            traceparent: None,
            baggage: None,
            output_is_input: false,
        };
        let full = engine.observers.report(
            host,
            DEFAULT_NAMESPACE,
            &super::RoutedAnswers::default(),
            given.iter().map(item).collect::<Vec<_>>().into(),
        );
        assert!(full.is_empty());
        until(|| engine.observers.in_flight("obs") == Some((1, 2))).await;
        {
            let observer = engine.observers.get("obs").unwrap();
            let flow = observer.flow.write();
            for pending in &flow.queue {
                assert!(Arc::ptr_eq(&pending.event.function, &set.function));
            }
            assert_eq!(
                flow.queue[0].event.invocation_id,
                super::InvocationId::Uuid(uuid)
            );
            assert_eq!(
                flow.queue[1].event.invocation_id,
                super::InvocationId::Reported(given[2].clone())
            );
        }
        gate.add_permits(3);
        for id in &given {
            let envelope = test_support::next(&mut delivered).await;
            assert_eq!(envelope["invocation_id"], id.as_str());
            assert_eq!(envelope["function_id"], "test::src");
        }
    }

    /// The events of an engine-hosted source share one `caller`, the
    /// engine's, built once rather than for each event.
    #[tokio::test]
    async fn an_engine_sources_events_share_the_engines_caller() {
        let engine = Arc::new(Engine::new());
        super::super::events::register_event_source(&engine, "test::events", "events", json!({}));
        let (gate, mut delivered, _socket) = one_at_a_time(&engine, "test::events").await;
        for n in 0..3 {
            engine
                .observers
                .emit(DEFAULT_NAMESPACE, "test::events", || {
                    Some(json!({ "n": n }))
                });
        }
        until(|| engine.observers.in_flight("obs") == Some((1, 2))).await;
        {
            let observer = engine.observers.get("obs").unwrap();
            let flow = observer.flow.write();
            let callers: Vec<Arc<Value>> = flow
                .queue
                .iter()
                .map(|pending| pending.event.caller.clone().expect("captured"))
                .collect();
            assert!(Arc::ptr_eq(&callers[0], &callers[1]));
            assert_eq!(*callers[0], json!({ "worker_name": "iii-engine" }));
        }
        gate.add_permits(3);
        for n in 0..3 {
            let envelope = test_support::next(&mut delivered).await;
            assert_eq!(envelope["caller"], json!({ "worker_name": "iii-engine" }));
            assert_eq!(envelope["input"], json!({ "n": n }));
        }
    }

    /// A payload held as text costs the queues and the evaluators its
    /// text's length, and the value decoded from it as well once an
    /// activation decoded one. The filters read a value decoded for them
    /// alone, which the payload does not keep, and none when they do not
    /// read it.
    #[test]
    fn a_payload_held_as_text_costs_its_length() {
        let large = json!({ "big": "x".repeat(4096), "n": [1, 2, 3] });
        let payload = super::Payload::of(&large);
        let super::Payload::Text(text) = &payload else {
            panic!("kept as text");
        };
        let length = 24 + text.text.len();
        assert_eq!(payload.size(), length);
        let mut kept = Arc::into_inner(event(0)).unwrap();
        let small = super::approx_size(&json!({ "n": 0 }));
        kept.input = Some(super::Payload::of(&large));
        assert_eq!(kept.cost(), event(0).cost() - small + length);

        assert!(payload.filtered(false).is_none());
        let read = payload.filtered(true).unwrap();
        assert!(matches!(read, std::borrow::Cow::Owned(_)));
        assert_eq!(*read, large);
        assert!(text.value.get().is_none(), "not kept for the filters");

        assert_eq!(payload.value().map(|value| &**value), Some(&large));
        assert_eq!(payload.size(), length + super::approx_size(&large));
        let read = payload.filtered(true).unwrap();
        assert!(matches!(read, std::borrow::Cow::Borrowed(_)));
    }

    /// A call that keeps a large input as its JSON text makes an event that
    /// holds the text alone while it waits in a queue, whether the
    /// observers' filters read the input or not: the filters match as they
    /// would on the value, and the in-process target gets the value as the
    /// caller sent it, without the engine's caller stamp.
    #[tokio::test]
    async fn a_queued_event_kept_as_text_holds_the_text_alone() {
        let engine = Arc::new(Engine::new());
        engine.observers.set_config(super::super::ObserversConfig {
            default_max_in_flight: 1,
            ..Default::default()
        });
        source(&engine);
        let (gate, mut delivered) = gated(&engine, "test::held");
        let (worker, _socket) = test_support::worker(&engine);
        for (id, filters) in [("a", None), ("b", Some(json!({ "input.k": { "$eq": 1 } })))] {
            let request = test_support::request(id, "test::src", "test::held", filters);
            test_support::observe_request(&engine, &worker, request).await;
        }
        let body = |k: u64| json!({ "k": k, "big": "x".repeat(4096), "_caller_worker_id": "w" });
        for k in [1, 2, 1] {
            complete_call(&engine, body(k));
        }
        // Each runs the first event; `a` queues the other two, `b` the
        // third alone, whose `input.k` matches.
        until(|| {
            engine.observers.in_flight("a") == Some((1, 2))
                && engine.observers.in_flight("b") == Some((1, 1))
        })
        .await;
        for id in ["a", "b"] {
            let observer = engine.observers.get(id).unwrap();
            for pending in &observer.flow.write().queue {
                let Some(super::Payload::Text(text)) = &pending.event.input else {
                    panic!("kept as text");
                };
                assert!(
                    text.value.get().is_none(),
                    "{id}: a queued event holds the text alone"
                );
            }
        }
        gate.add_permits(5);
        let mut seen = Vec::new();
        for _ in 0..5 {
            let input = test_support::next(&mut delivered).await;
            let k = input["k"].as_u64().unwrap();
            assert_eq!(input, json!({ "k": k, "big": "x".repeat(4096) }));
            seen.push(k);
        }
        seen.sort_unstable();
        assert_eq!(seen, [1, 1, 1, 1, 2]);
    }

    /// A report's payloads stay the text its host sent: the queued event
    /// holds no value, a worker's activation sends that text (less a
    /// top-level caller stamp, the other members as written) and an
    /// in-process target gets the value it decodes to, decoded once and
    /// shared with `output_is_input`'s output. The queues charge the text,
    /// once. An item that does not decode is set aside alone.
    #[tokio::test]
    async fn a_reports_payloads_stay_the_text_its_host_sent() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let (gate, mut delivered) = gated(&engine, "test::held");
        let (worker, _socket) = test_support::worker(&engine);
        let mut request = test_support::request("obs", "test::src", "test::held", None);
        request.max_in_flight = Some(1);
        // The envelope: the input and the output.
        request.source.raw_input = false;
        test_support::observe_request(&engine, &worker, request).await;
        let host = Uuid::new_v4();
        engine.function_owners.insert(
            (DEFAULT_NAMESPACE.to_string(), "test::src".to_string()),
            (host, crate::engine::InvocationKind::Regular),
        );
        let report = |items: &[String]| {
            let frame = format!(
                r#"{{"type":"invocationsobserved","items":[{}]}}"#,
                items.join(",")
            );
            let Ok(crate::protocol::Message::InvocationsObserved { items }) =
                crate::protocol::Message::from_frame(&frame)
            else {
                panic!("expected InvocationsObserved");
            };
            items
        };
        let item = |fields: &str| {
            format!(
                r#"{{"function_id":"test::src","invocation_id":"{}","matched":["obs"],"status":"success","started_at_ms":1,"duration_ms":0.5,{fields}}}"#,
                Uuid::new_v4()
            )
        };
        let answered = super::RoutedAnswers::default();
        // The first takes the observer's only slot: the rest queue.
        let first = report(&[item(r#""input":{"n":0}"#)]);
        assert!(
            engine
                .observers
                .report(host, DEFAULT_NAMESPACE, &answered, first)
                .is_empty()
        );
        until(|| engine.observers.in_flight("obs") == Some((1, 0))).await;

        let stamped = r#"{"table": "orders", "_caller_worker_id": "w-9", "op": "insert"}"#;
        let raw = r#"{"z": [1.50, null], "a": "\u00e9"}"#;
        let items = report(&[
            item(&format!(r#""input":{stamped},"output_is_input":true"#)),
            item(&format!(
                r#""input":{raw},"output":{{"ok": true}},"metadata":{{"tenant": "t"}}"#
            )),
            r#""not an item""#.to_string(),
        ]);
        assert_eq!(items.rejected.len(), 1, "set aside alone");
        assert!(
            engine
                .observers
                .report(host, DEFAULT_NAMESPACE, &answered, items)
                .is_empty()
        );
        until(|| engine.observers.in_flight("obs") == Some((1, 2))).await;

        let observer = engine.observers.get("obs").unwrap();
        let events: Vec<Arc<super::Completed>> = observer
            .flow
            .write()
            .queue
            .iter()
            .map(|pending| pending.event.clone())
            .collect();
        let text = |payload: &Option<super::Payload>| match payload {
            Some(super::Payload::Text(held)) => Some(held.text.clone()),
            _ => None,
        };
        let decoded = |payload: &Option<super::Payload>| matches!(payload, Some(super::Payload::Text(held)) if held.value.get().is_some());
        let held = |needle: &str| {
            events
                .iter()
                .find(|event| text(&event.input).is_some_and(|t| t.contains(needle)))
                .unwrap()
                .clone()
        };
        let (echoed, plain) = (held("table"), held("\\u00e9"));
        for event in [&echoed, &plain] {
            assert!(
                !decoded(&event.input) && !decoded(&event.output),
                "no value"
            );
            assert!(event.encoded.get().is_none(), "nothing encoded");
        }
        let input = text(&echoed.input).unwrap();
        assert_eq!(&*input, r#"{"table":"orders","op":"insert"}"#);
        assert!(
            echoed.output_is_input && echoed.output.is_none(),
            "held once"
        );
        assert_eq!(text(&plain.input).as_deref(), Some(raw));
        assert_eq!(text(&plain.metadata).as_deref(), Some(r#"{"tenant": "t"}"#));
        let caller = echoed.caller.as_deref().map_or(0, super::approx_size);
        assert_eq!(
            echoed.cost(),
            super::EVENT_OVERHEAD
                + echoed.function.0.len()
                + echoed.function.1.len()
                + echoed.invocation_id.len()
                + caller
                + 24
                + input.len(),
            "its text, once"
        );

        for event in [&echoed, &plain] {
            let mut sent = String::new();
            super::project_encoded(&observer, event, "a-1").write_to(&mut sent);
            assert_eq!(
                serde_json::from_str::<Value>(&sent).unwrap(),
                super::project(&observer, event, "a-1")
            );
            if Arc::ptr_eq(event, &plain) {
                assert!(sent.contains(&format!(r#""input":{raw}"#)), "{sent}");
            }
        }
        assert!(decoded(&echoed.input));
        assert!(std::ptr::eq(
            echoed.input_value().unwrap(),
            echoed.output_value().unwrap()
        ));
        drop(events);

        gate.add_permits(3);
        let mut envelopes = Vec::new();
        for _ in 0..3 {
            envelopes.push(test_support::next(&mut delivered).await);
        }
        let row = json!({ "table": "orders", "op": "insert" });
        assert!(
            envelopes
                .iter()
                .any(|e| e["input"] == row && e["output"] == row),
            "{envelopes:?}"
        );
        assert!(
            envelopes
                .iter()
                .any(|e| e["input"] == json!({ "z": [1.5, null], "a": "\u{e9}" })
                    && e["output"] == json!({ "ok": true })),
            "{envelopes:?}"
        );
    }

    /// A payload's text loses a top-level caller stamp however its key is
    /// written, and nothing else, as its value would; text that cannot hold
    /// one is not read.
    #[test]
    fn the_caller_stamp_is_cut_from_a_payloads_text() {
        for (stamped, cut) in [
            (
                r#"{"a":1,"_caller_worker_id":"w","b":{"_caller_worker_id":"x"}}"#,
                r#"{"a":1,"b":{"_caller_worker_id":"x"}}"#,
            ),
            (
                r#"{"_caller\u005fworker_id":"w","\u00e9": [1, 2]}"#,
                "{\"\u{e9}\":[1, 2]}",
            ),
            (r#"{"_caller_worker_id":1,"_caller_worker_id":2}"#, "{}"),
        ] {
            let unstamped = super::unstamped_text(Arc::from(stamped));
            assert_eq!(&*unstamped, cut);
            assert_eq!(
                serde_json::from_str::<Value>(&unstamped).unwrap(),
                super::unstamped(serde_json::from_str(stamped).unwrap())
            );
        }
        for kept in [
            r#"{"b":{"_caller_worker_id":"x"}}"#,
            r#""_caller_worker_id""#,
            r#"["_caller_worker_id"]"#,
            r#"{"a": "\u00e9"}"#,
            r#"{"a": 1}"#,
        ] {
            let text: Arc<str> = Arc::from(kept);
            assert!(
                Arc::ptr_eq(&super::unstamped_text(text.clone()), &text),
                "{kept}"
            );
        }
    }
}
