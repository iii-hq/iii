pub mod api;
pub mod config;

pub use api::ManagementModule;

crate::register_module!(
    "modules::management::ManagementModule",
    <ManagementModule as crate::modules::core_module::CoreModule>::make_module,
    enabled_by_default = true
);
