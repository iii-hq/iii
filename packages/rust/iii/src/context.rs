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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::Value;

    use super::*;

    #[tokio::test]
    async fn get_context_returns_scoped_context() {
        let calls: Arc<Mutex<Vec<(String, Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_ref = calls.clone();
        let invoker = Arc::new(move |path: &str, params: Value| {
            calls_ref.lock().unwrap().push((path.to_string(), params));
        });

        let logger = Logger::new(
            Some(invoker),
            Some("trace-ctx".to_string()),
            Some("fn-ctx".to_string()),
        );

        with_context(Context { logger }, || async {
            let ctx = get_context();
            ctx.logger.info("inside", None);
        })
        .await;

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "logger.info");
        assert_eq!(calls[0].1["trace_id"], "trace-ctx");
        assert_eq!(calls[0].1["function_name"], "fn-ctx");
        assert_eq!(calls[0].1["message"], "inside");
    }
}
