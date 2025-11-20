pub struct Logger {}

impl Logger {
    pub fn info(&self, message: &str) {
        tracing::info!(message);
    }

    pub fn warn(&self, message: &str) {
        tracing::warn!(message);
    }

    pub fn error(&self, message: &str) {
        tracing::error!(message);
    }
}
