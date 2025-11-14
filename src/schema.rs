use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Schema {
    Array {
        items: Vec<Schema>,
    },
    Boolean,
    Integer,
    Object {
        properties: HashMap<String, Schema>,
        required: Vec<String>,
        #[serde(rename = "additionalProperties")]
        additional_properties: bool,
    },
    String {
        format: String,
    },
    Number {
        format: String,
    },
    Null,
    Any,
    Enum {
        values: Vec<String>,
    },
}
