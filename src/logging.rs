use std::sync::OnceLock;
use tracing_subscriber::{EnvFilter, fmt};

static TRACING: OnceLock<()> = OnceLock::new();

pub fn init_tracing() {
    TRACING.get_or_init(|| {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        fmt::Subscriber::builder()
            .with_env_filter(filter)
            .with_target(true)
            .compact()
            .init();
    });
}
