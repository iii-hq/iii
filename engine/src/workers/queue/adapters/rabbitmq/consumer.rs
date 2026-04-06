// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

#![cfg(feature = "rabbitmq")]

use lapin::{
    message::Delivery,
    types::{AMQPValue, FieldTable},
};

use super::naming::EXCHANGE_PREFIX;
use super::types::Job;

pub type Result<T> = std::result::Result<T, ConsumerError>;

#[derive(Debug)]
pub enum ConsumerError {
    Serialization(serde_json::Error),
}

impl std::fmt::Display for ConsumerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsumerError::Serialization(e) => write!(f, "Serialization error: {}", e),
        }
    }
}

impl std::error::Error for ConsumerError {}

impl From<serde_json::Error> for ConsumerError {
    fn from(err: serde_json::Error) -> Self {
        ConsumerError::Serialization(err)
    }
}

pub struct JobParser;

impl JobParser {
    pub fn parse_from_delivery(delivery: &Delivery) -> Result<Job> {
        let mut job: Job = serde_json::from_slice(&delivery.data)?;

        if let Some(headers) = &delivery.properties.headers()
            && let Some(attempts) = Self::extract_attempts(headers)?
        {
            job.attempts_made = attempts;
        }

        Ok(job)
    }

    fn extract_attempts(headers: &FieldTable) -> Result<Option<u32>> {
        let header_name = format!("x-{}-attempts", EXCHANGE_PREFIX);
        if let Some(value) = headers.inner().get(header_name.as_str()) {
            match value {
                AMQPValue::LongUInt(v) => Ok(Some(*v)),
                AMQPValue::ShortUInt(v) => Ok(Some(*v as u32)),
                AMQPValue::LongInt(v) => Ok(Some(*v as u32)),
                AMQPValue::ShortInt(v) => Ok(Some(*v as u32)),
                _ => Ok(None),
            }
        } else {
            Ok(None)
        }
    }
}
