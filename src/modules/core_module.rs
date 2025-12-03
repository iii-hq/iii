use crate::engine::Engine;

#[async_trait::async_trait]
pub trait CoreModule {
    async fn initialize(&self) -> Result<(), anyhow::Error>;
    fn register_functions(&self, engine: ::std::sync::Arc<Engine>);
}
