use crate::modules::{
    cron::structs::CronSchedulerAdapter,
    registry::{AdapterFuture, AdapterRegistration},
};

pub(crate) type CronAdapterFuture = AdapterFuture<dyn CronSchedulerAdapter>;
pub(crate) type CronAdapterRegistration = AdapterRegistration<dyn CronSchedulerAdapter>;

inventory::collect!(CronAdapterRegistration);
