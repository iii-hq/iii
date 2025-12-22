use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum LiteralValue {
    String(String),
    Number(f64),
    Boolean(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaProperty {
    pub schema: SchemaNode,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SchemaNode {
    String,
    Number,
    Boolean,
    Null,
    Array(Box<SchemaNode>),
    Object {
        properties: HashMap<String, SchemaProperty>,
        additional_properties: bool,
    },
    Union(Vec<SchemaNode>),
    Optional(Box<SchemaNode>),
    Literal(LiteralValue),
    Never,
    Unknown,
}

impl SchemaNode {
    pub fn make_optional(self) -> Self {
        match self {
            SchemaNode::Optional(_) => self,
            _ => SchemaNode::Optional(Box::new(self)),
        }
    }

    pub fn is_optional(&self) -> bool {
        matches!(self, SchemaNode::Optional(_))
    }
}


