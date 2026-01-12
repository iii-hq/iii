use crate::logger::Logger;

#[derive(Clone)]
pub struct Context {
    pub logger: Logger,
}

tokio::task_local! {
    static CONTEXT: Context;
}

pub async fn with_context<F, Fut, T>(context: Context, f: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    CONTEXT.scope(context, f()).await
}

pub fn get_context() -> Context {
    CONTEXT
        .try_with(|ctx| ctx.clone())
        .unwrap_or_else(|_| Context {
            logger: Logger::default(),
        })
}
