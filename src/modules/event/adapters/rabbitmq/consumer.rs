use lapin::{
    message::Delivery,
    types::{AMQPValue, FieldTable},
};

use super::types::Job;

pub type Result<T> = std::result::Result<T, ConsumerError>;

#[derive(Debug)]
pub enum ConsumerError {
    Serialization(serde_json::Error),
    #[allow(dead_code)]
    MissingHeader(&'static str),
    InvalidHeaderValue(String),
}

impl std::fmt::Display for ConsumerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsumerError::Serialization(e) => write!(f, "Serialization error: {}", e),
            ConsumerError::MissingHeader(header) => write!(f, "Missing header: {}", header),
            ConsumerError::InvalidHeaderValue(msg) => write!(f, "Invalid header value: {}", msg),
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
        if let Some(value) = headers.inner().get("x-iii-attempts") {
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

    #[allow(dead_code)]
    fn extract_max_attempts(headers: &FieldTable) -> Result<Option<u32>> {
        if let Some(value) = headers.inner().get("x-iii-max-attempts") {
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

    #[allow(dead_code)]
    fn extract_created_at(headers: &FieldTable) -> Result<Option<u64>> {
        if let Some(value) = headers.inner().get("x-iii-created-at") {
            match value {
                AMQPValue::LongString(s) => {
                    let s_str = std::str::from_utf8(s.as_bytes())
                        .map_err(|e| ConsumerError::InvalidHeaderValue(e.to_string()))?;
                    let timestamp = s_str
                        .parse::<u64>()
                        .map_err(|e| ConsumerError::InvalidHeaderValue(e.to_string()))?;
                    Ok(Some(timestamp))
                }
                _ => Ok(None),
            }
        } else {
            Ok(None)
        }
    }
}
