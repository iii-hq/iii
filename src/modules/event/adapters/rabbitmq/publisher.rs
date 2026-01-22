use std::sync::Arc;

use lapin::{
    Channel,
    options::*,
    types::{AMQPValue, FieldTable},
};

use super::naming::RabbitNames;
use super::types::Job;

pub type Result<T> = std::result::Result<T, PublisherError>;

#[derive(Debug)]
pub enum PublisherError {
    Lapin(lapin::Error),
    Serialization(serde_json::Error),
}

impl std::fmt::Display for PublisherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PublisherError::Lapin(e) => write!(f, "RabbitMQ error: {}", e),
            PublisherError::Serialization(e) => write!(f, "Serialization error: {}", e),
        }
    }
}

impl std::error::Error for PublisherError {}

impl From<lapin::Error> for PublisherError {
    fn from(err: lapin::Error) -> Self {
        PublisherError::Lapin(err)
    }
}

impl From<serde_json::Error> for PublisherError {
    fn from(err: serde_json::Error) -> Self {
        PublisherError::Serialization(err)
    }
}

pub struct Publisher {
    channel: Arc<Channel>,
}

impl Publisher {
    pub fn new(channel: Arc<Channel>) -> Self {
        Self { channel }
    }

    pub async fn publish(&self, topic: &str, job: &Job) -> Result<()> {
        let names = RabbitNames::new(topic);
        let headers = self.build_headers(job);
        self.publish_to_exchange(&names.exchange(), topic, job, Some(headers))
            .await
    }

    pub async fn requeue(&self, topic: &str, job: &Job) -> Result<()> {
        self.publish(topic, job).await
    }

    pub async fn publish_to_dlq(&self, topic: &str, job: &Job, error: &str) -> Result<()> {
        let names = RabbitNames::new(topic);
        let payload = serde_json::to_vec(&serde_json::json!({
            "job": job,
            "error": error,
            "exhausted_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        }))?;

        let properties = lapin::BasicProperties::default()
            .with_content_type("application/json".into())
            .with_delivery_mode(2);

        self.channel
            .basic_publish(
                "",
                &names.dlq(),
                BasicPublishOptions::default(),
                &payload,
                properties,
            )
            .await?
            .await?;

        Ok(())
    }

    async fn publish_to_exchange(
        &self,
        exchange: &str,
        routing_key: &str,
        job: &Job,
        headers: Option<FieldTable>,
    ) -> Result<()> {
        let payload = serde_json::to_vec(job)?;

        let properties = lapin::BasicProperties::default()
            .with_content_type("application/json".into())
            .with_delivery_mode(2)
            .with_headers(headers.unwrap_or_default());

        self.channel
            .basic_publish(
                exchange,
                routing_key,
                BasicPublishOptions::default(),
                &payload,
                properties,
            )
            .await?
            .await?;

        Ok(())
    }

    fn build_headers(&self, job: &Job) -> FieldTable {
        let mut headers = FieldTable::default();
        headers.insert(
            "x-iii-job-id".into(),
            AMQPValue::LongString(job.id.clone().into()),
        );
        headers.insert(
            "x-iii-attempts".into(),
            AMQPValue::LongUInt(job.attempts_made),
        );
        headers.insert(
            "x-iii-max-attempts".into(),
            AMQPValue::LongUInt(job.max_attempts),
        );
        headers.insert(
            "x-iii-created-at".into(),
            AMQPValue::LongString(job.created_at.to_string().into()),
        );
        headers
    }
}
