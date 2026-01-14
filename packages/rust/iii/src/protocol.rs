use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
    RegisterTriggerType {
        id: String,
        description: String,
    },
    RegisterTrigger {
        id: String,
        trigger_type: String,
        function_path: String,
        config: Value,
    },
    TriggerRegistrationResult {
        id: String,
        trigger_type: String,
        function_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
    },
    UnregisterTrigger {
        id: String,
        trigger_type: String,
    },
    RegisterFunction {
        function_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_format: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_format: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    InvokeFunction {
        invocation_id: Option<Uuid>,
        function_path: String,
        data: Value,
    },
    InvocationResult {
        invocation_id: Uuid,
        function_path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
    },
    RegisterService {
        id: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    FunctionsAvailable {
        functions: Vec<FunctionMessage>,
    },
    ListFunctions,
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterTriggerTypeMessage {
    pub id: String,
    pub description: String,
}

impl RegisterTriggerTypeMessage {
    pub fn to_message(&self) -> Message {
        Message::RegisterTriggerType {
            id: self.id.clone(),
            description: self.description.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterTriggerMessage {
    pub id: String,
    pub trigger_type: String,
    pub function_path: String,
    pub config: Value,
}

impl RegisterTriggerMessage {
    pub fn to_message(&self) -> Message {
        Message::RegisterTrigger {
            id: self.id.clone(),
            trigger_type: self.trigger_type.clone(),
            function_path: self.function_path.clone(),
            config: self.config.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnregisterTriggerMessage {
    pub id: String,
    pub trigger_type: String,
}

impl UnregisterTriggerMessage {
    pub fn to_message(&self) -> Message {
        Message::UnregisterTrigger {
            id: self.id.clone(),
            trigger_type: self.trigger_type.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterFunctionMessage {
    pub function_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl RegisterFunctionMessage {
    pub fn to_message(&self) -> Message {
        Message::RegisterFunction {
            function_path: self.function_path.clone(),
            description: self.description.clone(),
            request_format: self.request_format.clone(),
            response_format: self.response_format.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterServiceMessage {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl RegisterServiceMessage {
    pub fn to_message(&self) -> Message {
        Message::RegisterService {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionMessage {
    pub function_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_function_to_message_and_serializes_type() {
        let msg = RegisterFunctionMessage {
            function_path: "functions.echo".to_string(),
            description: Some("Echo function".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        };

        let message = msg.to_message();
        match &message {
            Message::RegisterFunction {
                function_path,
                description,
                ..
            } => {
                assert_eq!(function_path, "functions.echo");
                assert_eq!(description.as_deref(), Some("Echo function"));
            }
            _ => panic!("unexpected message variant"),
        }

        let serialized = serde_json::to_value(&message).unwrap();
        assert_eq!(serialized["type"], "registerfunction");
        assert_eq!(serialized["function_path"], "functions.echo");
        assert_eq!(serialized["description"], "Echo function");
    }
}
