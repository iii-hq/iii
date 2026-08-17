// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

#![cfg(feature = "rabbitmq")]

use crate::builtins::queue::escape_ns_segment;

pub const EXCHANGE_PREFIX: &str = "iii";

/// Separator that folds the subscribing namespace into a per-function
/// subscriber queue name.
///
/// A subscriber queue is `{prefix}.{topic}.{fid}@{ns}.queue`. `topic`, `fid`
/// and `ns` are all user-controlled and may contain `@`, so a bare `@` join is
/// NOT collision-free — `(fid="a@x", ns="y")` and `(fid="a", ns="x@y")` would
/// fold to the same queue and make two namespaces competing consumers of one
/// queue. Every `@` inside `topic`, `fid` and `ns` is therefore escaped (via
/// [`escape_ns_segment`]) before joining, leaving exactly one unescaped `@` as
/// the true namespace boundary. The fold is thus injective over
/// `(topic, fid, ns)` w.r.t. the `@` dimension for any content. (The
/// `topic`/`fid` `.`-boundary ambiguity predates namespaces and is unchanged.)
pub const NS_SEP: char = '@';

pub struct RabbitNames {
    pub topic: String,
}

impl RabbitNames {
    pub fn new(topic: impl Into<String>) -> Self {
        Self {
            topic: topic.into(),
        }
    }

    pub fn exchange(&self) -> String {
        format!("{}.{}.exchange", EXCHANGE_PREFIX, self.topic)
    }

    pub fn queue(&self) -> String {
        format!("{}.{}.queue", EXCHANGE_PREFIX, self.topic)
    }

    pub fn function_queue(&self, namespace: &str, function_id: &str) -> String {
        format!(
            "{}.{}.{}{}{}.queue",
            EXCHANGE_PREFIX,
            escape_ns_segment(&self.topic, NS_SEP),
            escape_ns_segment(function_id, NS_SEP),
            NS_SEP,
            escape_ns_segment(namespace, NS_SEP)
        )
    }

    pub fn function_dlq(&self, namespace: &str, function_id: &str) -> String {
        format!(
            "{}.{}.{}{}{}.dlq",
            EXCHANGE_PREFIX,
            escape_ns_segment(&self.topic, NS_SEP),
            escape_ns_segment(function_id, NS_SEP),
            NS_SEP,
            escape_ns_segment(namespace, NS_SEP)
        )
    }

    pub fn dlq(&self) -> String {
        format!("{}.{}.dlq", EXCHANGE_PREFIX, self.topic)
    }
}

pub struct FnQueueNames {
    pub name: String,
}

impl FnQueueNames {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn exchange(&self) -> String {
        format!("{}.__fn_queue::{}", EXCHANGE_PREFIX, self.name)
    }

    pub fn queue(&self) -> String {
        format!("{}.__fn_queue::{}.queue", EXCHANGE_PREFIX, self.name)
    }

    pub fn retry_exchange(&self) -> String {
        format!("{}.__fn_queue::{}::retry", EXCHANGE_PREFIX, self.name)
    }

    pub fn retry_queue(&self) -> String {
        format!("{}.__fn_queue::{}::retry.queue", EXCHANGE_PREFIX, self.name)
    }

    pub fn dlq_exchange(&self) -> String {
        format!("{}.__fn_queue::{}::dlq", EXCHANGE_PREFIX, self.name)
    }

    pub fn dlq(&self) -> String {
        format!("{}.__fn_queue::{}::dlq.queue", EXCHANGE_PREFIX, self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rabbit_names() {
        let names = RabbitNames::new("user.created");

        assert_eq!(names.exchange(), "iii.user.created.exchange");
        assert_eq!(names.queue(), "iii.user.created.queue");
        assert_eq!(names.dlq(), "iii.user.created.dlq");
    }

    #[test]
    fn test_function_queue_includes_namespace() {
        let names = RabbitNames::new("user.created");
        // Same topic + function id, different namespaces -> distinct queues.
        assert_eq!(
            names.function_queue("orders", "handle::it"),
            "iii.user.created.handle::it@orders.queue"
        );
        assert_eq!(
            names.function_queue("analytics", "handle::it"),
            "iii.user.created.handle::it@analytics.queue"
        );
        assert_ne!(
            names.function_queue("orders", "handle::it"),
            names.function_queue("analytics", "handle::it")
        );
    }

    /// The `@`-fold must be injective even when `fid`/`ns` themselves contain
    /// `@`. Under a bare `@` join, `(fid="task@blue", ns="green")` and
    /// `(fid="task", ns="blue@green")` both produce
    /// `iii.T.task@blue@green.queue` — two namespaces colliding on one queue.
    /// Escaping each segment keeps them distinct. Reverting to a bare join makes
    /// this RED (equal names).
    #[test]
    fn test_function_queue_namespace_fold_is_collision_free() {
        let names = RabbitNames::new("T");
        let a = names.function_queue("green", "task@blue");
        let b = names.function_queue("blue@green", "task");
        assert_ne!(
            a, b,
            "distinct (fid, ns) tuples must not fold to the same queue name"
        );
        // The DLQ pairing must stay consistent with the queue.
        assert_ne!(
            names.function_dlq("green", "task@blue"),
            names.function_dlq("blue@green", "task"),
        );
    }

    #[test]
    fn test_function_dlq_includes_namespace() {
        let names = RabbitNames::new("user.created");
        assert_eq!(
            names.function_dlq("orders", "handle::it"),
            "iii.user.created.handle::it@orders.dlq"
        );
        assert_ne!(
            names.function_dlq("orders", "handle::it"),
            names.function_dlq("analytics", "handle::it")
        );
    }

    #[test]
    fn test_fn_queue_names_exchange() {
        let names = FnQueueNames::new("orders");
        assert_eq!(names.exchange(), "iii.__fn_queue::orders");
    }

    #[test]
    fn test_fn_queue_names_queue() {
        let names = FnQueueNames::new("orders");
        assert_eq!(names.queue(), "iii.__fn_queue::orders.queue");
    }

    #[test]
    fn test_fn_queue_names_retry_exchange() {
        let names = FnQueueNames::new("orders");
        assert_eq!(names.retry_exchange(), "iii.__fn_queue::orders::retry");
    }

    #[test]
    fn test_fn_queue_names_retry_queue() {
        let names = FnQueueNames::new("orders");
        assert_eq!(names.retry_queue(), "iii.__fn_queue::orders::retry.queue");
    }

    #[test]
    fn test_fn_queue_names_dlq_exchange() {
        let names = FnQueueNames::new("orders");
        assert_eq!(names.dlq_exchange(), "iii.__fn_queue::orders::dlq");
    }

    #[test]
    fn test_fn_queue_names_dlq() {
        let names = FnQueueNames::new("orders");
        assert_eq!(names.dlq(), "iii.__fn_queue::orders::dlq.queue");
    }

    #[test]
    fn test_fn_queue_names_with_dots() {
        let names = FnQueueNames::new("payment.processing");
        assert_eq!(names.exchange(), "iii.__fn_queue::payment.processing");
        assert_eq!(names.queue(), "iii.__fn_queue::payment.processing.queue");
        assert_eq!(
            names.retry_exchange(),
            "iii.__fn_queue::payment.processing::retry"
        );
        assert_eq!(
            names.retry_queue(),
            "iii.__fn_queue::payment.processing::retry.queue"
        );
        assert_eq!(
            names.dlq_exchange(),
            "iii.__fn_queue::payment.processing::dlq"
        );
        assert_eq!(names.dlq(), "iii.__fn_queue::payment.processing::dlq.queue");
    }
}
