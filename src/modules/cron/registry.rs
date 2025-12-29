use crate::modules::{
    cron::structs::CronSchedulerAdapter,
    registry::{AdapterFuture, AdapterRegistration},
};

pub type CronAdapterFuture = AdapterFuture<dyn CronSchedulerAdapter>;
pub type CronAdapterRegistration = AdapterRegistration<dyn CronSchedulerAdapter>;

inventory::collect!(CronAdapterRegistration);
