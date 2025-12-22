use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Result};
use swc_common::sync::Lrc;
use swc_common::{BytePos, FileName, SourceMap};
use swc_ecma_ast::{
    Decl, Expr, ExportDecl, KeyValueProp, Lit, ModuleDecl, ModuleItem, ObjectLit, Prop,
    PropOrSpread, VarDecl, VarDeclKind, VarDeclarator,
};
use swc_ecma_parser::{Parser, StringInput, Syntax, TsSyntax};

use super::{LanguageParser, ParsedFile};
use super::zod::ZodParser;
use crate::modules::typegen::schema::{SchemaNode, StepDefinition, StepType, StreamDefinition};

pub struct TypeScriptParser;

impl LanguageParser for TypeScriptParser {
    fn supports_extension(&self, ext: &str) -> bool {
        ext == "ts"
    }

    fn parse_file(&self, path: &Path, content: &str) -> Result<ParsedFile> {
        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(Lrc::new(FileName::Real(path.to_path_buf())), content.into());

        let syntax = Syntax::Typescript(TsSyntax {
            tsx: false,
            decorators: false,
            ..Default::default()
        });

        let src = fm.src.as_ref();
        let input = StringInput::new(src, BytePos(0), fm.end_pos);
        let mut parser = Parser::new(syntax, input, Default::default());
        let module = parser
            .parse_module()
            .map_err(|e| anyhow!("Failed to parse TypeScript: {:?}", e))?;

        let mut variable_schemas: HashMap<String, Expr> = HashMap::new();
        let mut steps = Vec::new();
        let mut streams = Vec::new();

        for item in &module.body {
            match item {
                ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(Decl::Var(var_decl))) => {
                    if let Some((name, expr)) = Self::extract_const_export_expr(var_decl) {
                        variable_schemas.insert(name, expr);
                    }
                }
                ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl { decl, .. })) => {
                    if let Decl::Var(var_decl) = decl {
                        if let Some((name, expr)) = Self::extract_const_export_expr(var_decl) {
                            if name != "config" {
                                variable_schemas.insert(name, expr);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        for item in &module.body {
            if let ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl { decl, .. })) = item {
                if let Decl::Var(var_decl) = decl {
                    if let Some((name, expr)) = Self::extract_const_export_expr(var_decl) {
                        if name == "config" {
                            if let Expr::Object(obj) = &expr {
                                if let Some(config_type) = Self::get_object_string_prop(obj, "type") {
                                    match config_type.as_str() {
                                        "api" => {
                                            if let Some(step) = Self::parse_api_step(obj, path, &variable_schemas)? {
                                                steps.push(step);
                                            }
                                        }
                                        "event" => {
                                            if let Some(step) = Self::parse_event_step(obj, path, &variable_schemas)? {
                                                steps.push(step);
                                            }
                                        }
                                        "cron" => {
                                            if let Some(step) = Self::parse_cron_step(obj, path)? {
                                                steps.push(step);
                                            }
                                        }
                                        _ => {}
                                    }
                                } else {
                                    if let Some(stream) = Self::parse_stream(obj, path, &variable_schemas)? {
                                        streams.push(stream);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(ParsedFile { steps, streams })
    }
}

impl TypeScriptParser {
    fn extract_const_export_expr(var_decl: &VarDecl) -> Option<(String, Expr)> {
        if var_decl.kind != VarDeclKind::Const {
            return None;
        }

        for decl in &var_decl.decls {
            if let VarDeclarator {
                name,
                init: Some(init),
                ..
            } = decl
            {
                if let swc_ecma_ast::Pat::Ident(ident) = name {
                    return Some((ident.id.sym.to_string(), (**init).clone()));
                }
            }
        }

        None
    }

    fn resolve_expr<'a>(expr: &'a Expr, variables: &'a HashMap<String, Expr>) -> &'a Expr {
        if let Expr::Ident(ident) = expr {
            if let Some(resolved) = variables.get(&ident.sym.to_string()) {
                return Self::resolve_expr(resolved, variables);
            }
        }
        expr
    }

    fn get_object_string_prop(obj: &ObjectLit, key: &str) -> Option<String> {
        for prop in &obj.props {
            if let PropOrSpread::Prop(prop) = prop {
                if let Prop::KeyValue(KeyValueProp {
                    key: prop_key,
                    value,
                }) = &**prop
                {
                    if Self::prop_key_matches(prop_key, key) {
                        if let Expr::Lit(Lit::Str(s)) = &**value {
                            return Some(s.value.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    fn get_object_expr_prop(obj: &ObjectLit, key: &str) -> Option<Expr> {
        for prop in &obj.props {
            if let PropOrSpread::Prop(prop) = prop {
                if let Prop::KeyValue(KeyValueProp {
                    key: prop_key,
                    value,
                }) = &**prop
                {
                    if Self::prop_key_matches(prop_key, key) {
                        return Some((**value).clone());
                    }
                }
            }
        }
        None
    }

    fn get_object_array_prop(obj: &ObjectLit, key: &str) -> Option<Vec<String>> {
        for prop in &obj.props {
            if let PropOrSpread::Prop(prop) = prop {
                if let Prop::KeyValue(KeyValueProp {
                    key: prop_key,
                    value,
                }) = &**prop
                {
                    if Self::prop_key_matches(prop_key, key) {
                        if let Expr::Array(arr) = &**value {
                            let items: Vec<String> = arr
                                .elems
                                .iter()
                                .filter_map(|elem| elem.as_ref())
                                .filter_map(|elem| {
                                    if let Expr::Lit(Lit::Str(s)) = &*elem.expr {
                                        Some(s.value.to_string())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            return Some(items);
                        }
                    }
                }
            }
        }
        None
    }

    fn prop_key_matches(prop_key: &swc_ecma_ast::PropName, key: &str) -> bool {
        match prop_key {
            swc_ecma_ast::PropName::Ident(ident) => ident.sym.as_ref() == key,
            swc_ecma_ast::PropName::Str(s) => s.value.as_ref() == key,
            _ => false,
        }
    }

    fn parse_api_step(obj: &ObjectLit, path: &Path, variables: &HashMap<String, Expr>) -> Result<Option<StepDefinition>> {
        let name = Self::get_object_string_prop(obj, "name");
        let method = Self::get_object_string_prop(obj, "method");
        let route_path = Self::get_object_string_prop(obj, "path");

        if let (Some(name), Some(method), Some(route_path)) = (name, method, route_path) {
            let body_schema = if let Some(expr) = Self::get_object_expr_prop(obj, "bodySchema") {
                let resolved = Self::resolve_expr(&expr, variables);
                ZodParser::parse_zod_expr(resolved).ok()
            } else {
                None
            };

            let response_schemas = Self::parse_response_schemas(obj, variables)?;
            let emits = Self::get_object_array_prop(obj, "emits").unwrap_or_default();

            Ok(Some(StepDefinition {
                name,
                step_type: StepType::Api {
                    method,
                    path: route_path,
                },
                file_path: path.to_path_buf(),
                body_schema,
                response_schemas,
                emits,
                subscribes: Vec::new(),
            }))
        } else {
            Ok(None)
        }
    }

    fn parse_event_step(obj: &ObjectLit, path: &Path, variables: &HashMap<String, Expr>) -> Result<Option<StepDefinition>> {
        let name = Self::get_object_string_prop(obj, "name");
        let subscribes = Self::get_object_array_prop(obj, "subscribes").unwrap_or_default();

        if let Some(name) = name {
            let body_schema = if let Some(expr) = Self::get_object_expr_prop(obj, "input") {
                let resolved = Self::resolve_expr(&expr, variables);
                ZodParser::parse_zod_expr(resolved).ok()
            } else {
                None
            };

            let emits = Self::get_object_array_prop(obj, "emits").unwrap_or_default();

            Ok(Some(StepDefinition {
                name,
                step_type: StepType::Event,
                file_path: path.to_path_buf(),
                body_schema,
                response_schemas: HashMap::new(),
                emits,
                subscribes,
            }))
        } else {
            Ok(None)
        }
    }

    fn parse_cron_step(obj: &ObjectLit, path: &Path) -> Result<Option<StepDefinition>> {
        let name = Self::get_object_string_prop(obj, "name");
        let expression = Self::get_object_string_prop(obj, "cron");

        if let (Some(name), Some(expression)) = (name, expression) {
            let emits = Self::get_object_array_prop(obj, "emits").unwrap_or_default();

            Ok(Some(StepDefinition {
                name,
                step_type: StepType::Cron { expression },
                file_path: path.to_path_buf(),
                body_schema: None,
                response_schemas: HashMap::new(),
                emits,
                subscribes: Vec::new(),
            }))
        } else {
            Ok(None)
        }
    }

    fn parse_stream(obj: &ObjectLit, path: &Path, variables: &HashMap<String, Expr>) -> Result<Option<StreamDefinition>> {
        let name = Self::get_object_string_prop(obj, "name");

        if let Some(name) = name {
            let schema = if let Some(expr) = Self::get_object_expr_prop(obj, "schema") {
                let resolved = Self::resolve_expr(&expr, variables);
                ZodParser::parse_zod_expr(resolved).ok().unwrap_or(SchemaNode::Unknown)
            } else {
                SchemaNode::Unknown
            };

            Ok(Some(StreamDefinition {
                name,
                file_path: path.to_path_buf(),
                schema,
            }))
        } else {
            Ok(None)
        }
    }

    fn parse_response_schemas(obj: &ObjectLit, variables: &HashMap<String, Expr>) -> Result<HashMap<u16, SchemaNode>> {
        let mut schemas = HashMap::new();

        if let Some(Expr::Object(response_obj)) = Self::get_object_expr_prop(obj, "responseSchema")
        {
            for prop in &response_obj.props {
                if let PropOrSpread::Prop(prop) = prop {
                    if let Prop::KeyValue(KeyValueProp { key, value }) = &**prop {
                        if let Some(status_code) = Self::parse_status_code(key) {
                            let resolved = Self::resolve_expr(value, variables);
                            if let Ok(schema) = ZodParser::parse_zod_expr(resolved) {
                                schemas.insert(status_code, schema);
                            }
                        }
                    }
                }
            }
        }

        Ok(schemas)
    }

    fn parse_status_code(key: &swc_ecma_ast::PropName) -> Option<u16> {
        match key {
            swc_ecma_ast::PropName::Ident(ident) => ident.sym.parse().ok(),
            swc_ecma_ast::PropName::Str(s) => s.value.parse().ok(),
            swc_ecma_ast::PropName::Num(n) => Some(n.value as u16),
            _ => None,
        }
    }
}

