pub mod api;
pub mod config;

pub use api::ManagementModule;

crate::register_module!(
    "modules::management::ManagementModule",
    ManagementModule,
    enabled_by_default = true
);
