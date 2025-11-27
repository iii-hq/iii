#[async_trait::async_trait]
pub trait CoreModule {
    async fn initialize(&self) -> Result<(), anyhow::Error>;
}
