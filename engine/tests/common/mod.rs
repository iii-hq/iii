// Every test binary compiles this module but uses only some of its helpers.
#![allow(dead_code)]

pub mod http_helpers;
pub mod queue_helpers;

#[cfg(feature = "rabbitmq")]
pub mod rabbitmq_helpers;
