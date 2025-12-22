use std::collections::HashMap;

use anyhow::{anyhow, Result};
use swc_ecma_ast::{CallExpr, Callee, Expr, Ident, KeyValueProp, Lit, MemberExpr, MemberProp, ObjectLit, Prop, PropOrSpread};

use crate::modules::typegen::schema::{LiteralValue, SchemaNode, SchemaProperty};

pub struct ZodParser;

impl ZodParser {
    pub fn parse_zod_expr(expr: &Expr) -> Result<SchemaNode> {
        match expr {
            Expr::Call(call) => Self::parse_zod_call(call),
            Expr::Member(member) => Self::parse_zod_member(member),
            Expr::Ident(ident) => Self::parse_zod_ident(ident),
            _ => Ok(SchemaNode::Unknown),
        }
    }

    fn parse_zod_call(call: &CallExpr) -> Result<SchemaNode> {
        let callee_name = Self::get_callee_chain(call)?;

        if callee_name.ends_with("optional") {
            if let Callee::Expr(expr) = &call.callee {
                if let Expr::Member(member) = &**expr {
                    let base = Self::parse_zod_expr(&member.obj)?;
                    return Ok(base.make_optional());
                }
            }
            if let Some(arg) = call.args.first() {
                let inner = Self::parse_zod_expr(&arg.expr)?;
                return Ok(SchemaNode::Optional(Box::new(inner)));
            }
            return Ok(SchemaNode::Unknown);
        }

        if callee_name.contains("string") {
            return Ok(SchemaNode::String);
        }

        if callee_name.contains("number") {
            return Ok(SchemaNode::Number);
        }

        if callee_name.contains("boolean") {
            return Ok(SchemaNode::Boolean);
        }

        if callee_name.contains("null") {
            return Ok(SchemaNode::Null);
        }

        if callee_name.contains("array") {
            if let Some(arg) = call.args.first() {
                let inner = Self::parse_zod_expr(&arg.expr)?;
                return Ok(SchemaNode::Array(Box::new(inner)));
            }
            return Ok(SchemaNode::Array(Box::new(SchemaNode::Unknown)));
        }

        if callee_name.contains("object") {
            if let Some(arg) = call.args.first() {
                if let Expr::Object(obj) = &*arg.expr {
                    return Self::parse_zod_object(obj);
                }
            }
            return Ok(SchemaNode::Object {
                properties: HashMap::new(),
                additional_properties: false,
            });
        }

        if callee_name.contains("union") {
            if let Some(arg) = call.args.first() {
                if let Expr::Array(arr) = &*arg.expr {
                    let variants: Result<Vec<_>> = arr
                        .elems
                        .iter()
                        .filter_map(|elem| elem.as_ref())
                        .map(|elem| Self::parse_zod_expr(&elem.expr))
                        .collect();
                    return Ok(SchemaNode::Union(variants?));
                }
            }
            return Ok(SchemaNode::Unknown);
        }

        if callee_name.contains("literal") {
            if let Some(arg) = call.args.first() {
                if let Expr::Lit(lit) = &*arg.expr {
                    return Ok(SchemaNode::Literal(Self::parse_literal(lit)?));
                }
            }
            return Ok(SchemaNode::Unknown);
        }

        Ok(SchemaNode::Unknown)
    }

    fn parse_zod_member(member: &MemberExpr) -> Result<SchemaNode> {
        if let MemberProp::Ident(ident) = &member.prop {
            if ident.sym.as_ref() == "optional" {
                let base = Self::parse_zod_expr(&member.obj)?;
                return Ok(base.make_optional());
            }
        }

        Self::parse_zod_expr(&member.obj)
    }

    fn parse_zod_ident(_ident: &Ident) -> Result<SchemaNode> {
        Ok(SchemaNode::Unknown)
    }

    fn parse_zod_object(obj: &ObjectLit) -> Result<SchemaNode> {
        let mut properties = HashMap::new();

        for prop in &obj.props {
            if let PropOrSpread::Prop(prop) = prop {
                if let Prop::KeyValue(KeyValueProp { key, value }) = &**prop {
                    let key_name = Self::get_prop_key_name(key)?;
                    
                    match Self::parse_zod_expr(value) {
                        Ok(schema) => {
                            let optional = schema.is_optional();
                            let schema = if optional {
                                if let SchemaNode::Optional(inner) = schema {
                                    *inner
                                } else {
                                    schema
                                }
                            } else {
                                schema
                            };

                            properties.insert(
                                key_name,
                                SchemaProperty { schema, optional },
                            );
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse Zod schema for key '{}': {:?}", key_name, e);
                            properties.insert(
                                key_name,
                                SchemaProperty { 
                                    schema: SchemaNode::Unknown, 
                                    optional: false 
                                },
                            );
                        }
                    }
                }
            }
        }

        Ok(SchemaNode::Object {
            properties,
            additional_properties: false,
        })
    }

    fn get_prop_key_name(key: &swc_ecma_ast::PropName) -> Result<String> {
        match key {
            swc_ecma_ast::PropName::Ident(ident) => Ok(ident.sym.to_string()),
            swc_ecma_ast::PropName::Str(s) => Ok(s.value.to_string()),
            _ => Err(anyhow!("Unsupported property key type")),
        }
    }

    fn get_callee_chain(call: &CallExpr) -> Result<String> {
        match &call.callee {
            Callee::Expr(expr) => Self::get_expr_chain(expr),
            _ => Ok(String::new()),
        }
    }

    fn get_expr_chain(expr: &Expr) -> Result<String> {
        match expr {
            Expr::Member(member) => {
                let base = Self::get_expr_chain(&member.obj)?;
                if let MemberProp::Ident(ident) = &member.prop {
                    if base.is_empty() {
                        Ok(ident.sym.to_string())
                    } else {
                        Ok(format!("{}.{}", base, ident.sym))
                    }
                } else {
                    Ok(base)
                }
            }
            Expr::Ident(ident) => Ok(ident.sym.to_string()),
            Expr::Call(call) => Self::get_callee_chain(call),
            _ => Ok(String::new()),
        }
    }

    fn parse_literal(lit: &Lit) -> Result<LiteralValue> {
        match lit {
            Lit::Str(s) => Ok(LiteralValue::String(s.value.to_string())),
            Lit::Num(n) => Ok(LiteralValue::Number(n.value)),
            Lit::Bool(b) => Ok(LiteralValue::Boolean(b.value)),
            _ => Err(anyhow!("Unsupported literal type")),
        }
    }
}

