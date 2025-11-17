use crate::protocol::*;
use dashmap::DashMap;
use futures::Future;
use serde_json::Value;
use std::{pin::Pin, sync::Arc};
use uuid::Uuid;

pub struct Function {
    pub handler: Box<
        dyn Fn(
                Option<Uuid>,
                serde_json::Value,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<Option<serde_json::Value>, ErrorBody>>
                        + Send,
                >,
            > + Send
            + Sync,
    >,
    pub _function_path: String,
    pub _description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
}

impl From<&Function> for FunctionMessage {
    fn from(func: &Function) -> Self {
        FunctionMessage {
            function_path: func._function_path.clone(),
            description: func._description.clone(),
            request_format: func.request_format.clone(),
            response_format: func.response_format.clone(),
        }
    }
}

pub trait FunctionHandler {
    fn handle_function<'a>(
        &'a self,
        invocation_id: Option<Uuid>,
        function_path: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>, ErrorBody>> + Send + 'a>>;
}

#[derive(Default)]
pub struct FunctionsRegistry {
    pub functions: Arc<DashMap<String, Function>>,
}

impl FunctionsRegistry {
    pub fn new() -> Self {
        Self {
            functions: Arc::new(DashMap::new()),
        }
    }

    pub fn insert(&self, function_path: String, function: Function) {
        self.functions.insert(function_path, function);
    }

    pub fn remove(&self, function_path: &str) {
        self.functions.remove(function_path);
    }

    pub fn get(
        &self,
        function_path: &str,
    ) -> Option<dashmap::mapref::one::Ref<'_, String, Function>> {
        self.functions.get(function_path)
    }

    pub fn iter(&self) -> dashmap::iter::Iter<'_, String, Function> {
        self.functions.iter()
    }
}
