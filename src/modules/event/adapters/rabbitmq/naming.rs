// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

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
}
