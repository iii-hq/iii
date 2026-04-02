// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

#![cfg(feature = "rabbitmq")]

pub const EXCHANGE_PREFIX: &str = "iii";

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
