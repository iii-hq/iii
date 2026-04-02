// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, format_ident, quote};
use syn::{ItemFn, Meta, Token, parse::Parser, parse_macro_input, punctuated::Punctuated};

type AttrArgs = Punctuated<Meta, Token![,]>;

fn extract(args: &Punctuated<Meta, Token![,]>, name: &str) -> String {
    for meta in args {
        if let Meta::NameValue(nv) = meta
            && nv.path.is_ident(name)
            && let syn::Expr::Lit(expr_lit) = &nv.value
            && let syn::Lit::Str(s) = &expr_lit.lit
        {
            return s.value();
        }
    }
    panic!("Missing required attribute: {} = \"...\"", name);
}

fn extract_optional(args: &Punctuated<Meta, Token![,]>, name: &str) -> Option<String> {
    for meta in args {
        if let Meta::NameValue(nv) = meta
            && nv.path.is_ident(name)
            && let syn::Expr::Lit(expr_lit) = &nv.value
            && let syn::Lit::Str(s) = &expr_lit.lit
        {
            return Some(s.value());
        }
    }

    None
}

fn needs_serialization(return_type: &syn::Type) -> bool {
    let type_path = match return_type {
        syn::Type::Path(type_path) => type_path,
        _ => return false,
    };

    let segment = match type_path.path.segments.last() {
        Some(seg) => seg,
        None => return false,
    };

    if segment.ident != "FunctionResult" {
        return false;
    }

    let args = match &segment.arguments {
        syn::PathArguments::AngleBracketed(args) => args,
        _ => return false,
    };

    let success_ty = match args.args.first() {
        Some(syn::GenericArgument::Type(ty)) => ty,
        _ => return false,
    };

    // Check if success_ty is Option<Value> or Option<serde_json::Value>
    let opt_path = match success_ty {
        syn::Type::Path(opt_path) => opt_path,
        _ => return true, // Not Option, needs serialization
    };

    let opt_seg = match opt_path.path.segments.last() {
        Some(seg) => seg,
        None => return true, // Not Option, needs serialization
    };

    if opt_seg.ident != "Option" {
        return true; // Not Option, needs serialization
    }

    let opt_args = match &opt_seg.arguments {
        syn::PathArguments::AngleBracketed(args) => args,
        _ => return true, // Not Option, needs serialization
    };

    let inner_ty = match opt_args.args.first() {
        Some(syn::GenericArgument::Type(ty)) => ty,
        _ => return true, // Option with no type arg, needs serialization
    };

    let val_path = match inner_ty {
        syn::Type::Path(val_path) => val_path,
        _ => return true, // Not Value type, needs serialization
    };

    // Check if it's Value (could be serde_json::Value or just Value)
    let is_value = val_path
        .path
        .segments
        .last()
        .map(|seg| seg.ident == "Value")
        .unwrap_or(false);

    !is_value // If it's Option<Value>, no serialization needed; otherwise, needs serialization
}

/// Extracts the success type `T` from `FunctionResult<T, E>`.
///
/// Returns `None` if:
/// - The return type is not `FunctionResult<T, E>`
/// - `T` is `Option<Value>` or `Option<serde_json::Value>` (raw JSON, no schema)
/// - `T` is `()`
///
/// Returns `Some(token_stream)` with the success type otherwise.
fn extract_success_type(return_type: &syn::Type) -> Option<TokenStream2> {
    let type_path = match return_type {
        syn::Type::Path(type_path) => type_path,
        _ => return None,
    };

    let segment = type_path.path.segments.last()?;

    if segment.ident != "FunctionResult" {
        return None;
    }

    let args = match &segment.arguments {
        syn::PathArguments::AngleBracketed(args) => args,
        _ => return None,
    };

    let success_ty = match args.args.first() {
        Some(syn::GenericArgument::Type(ty)) => ty,
        _ => return None,
    };

    // Check for unit type ()
    if let syn::Type::Tuple(tuple) = success_ty
        && tuple.elems.is_empty()
    {
        return None;
    }

    // Check if it's Option<Value> or Option<serde_json::Value>
    if let syn::Type::Path(opt_path) = success_ty
        && let Some(opt_seg) = opt_path.path.segments.last()
        && opt_seg.ident == "Option"
        && let syn::PathArguments::AngleBracketed(opt_args) = &opt_seg.arguments
        && let Some(syn::GenericArgument::Type(syn::Type::Path(val_path))) = opt_args.args.first()
        && val_path
            .path
            .segments
            .last()
            .map(|seg| seg.ident == "Value")
            .unwrap_or(false)
    {
        return None; // Option<Value> -> no schema
    }

    Some(quote! { #success_ty })
}

fn type_contains_ident(ty: &syn::Type, name: &str) -> bool {
    match ty {
        syn::Type::Path(type_path) => type_path.path.segments.iter().any(|seg| {
            if seg.ident == name {
                return true;
            }
            if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                args.args.iter().any(|arg| {
                    if let syn::GenericArgument::Type(inner_ty) = arg {
                        type_contains_ident(inner_ty, name)
                    } else {
                        false
                    }
                })
            } else {
                false
            }
        }),
        _ => false,
    }
}

/// Checks whether `ty` is exactly `Option<Arc<Session>>`, tolerating
/// fully-qualified paths such as `std::sync::Arc` or `::std::sync::Arc`.
fn is_option_arc_session(ty: &syn::Type) -> bool {
    let type_path = match ty {
        syn::Type::Path(tp) => tp,
        _ => return false,
    };

    let option_seg = match type_path.path.segments.last() {
        Some(seg) if seg.ident == "Option" => seg,
        _ => return false,
    };

    let option_args = match &option_seg.arguments {
        syn::PathArguments::AngleBracketed(args) if args.args.len() == 1 => args,
        _ => return false,
    };

    let arc_ty = match option_args.args.first() {
        Some(syn::GenericArgument::Type(syn::Type::Path(tp))) => tp,
        _ => return false,
    };

    let arc_seg = match arc_ty.path.segments.last() {
        Some(seg) if seg.ident == "Arc" => seg,
        _ => return false,
    };

    let arc_args = match &arc_seg.arguments {
        syn::PathArguments::AngleBracketed(args) if args.args.len() == 1 => args,
        _ => return false,
    };

    let session_ty = match arc_args.args.first() {
        Some(syn::GenericArgument::Type(syn::Type::Path(tp))) => tp,
        _ => return false,
    };

    session_ty
        .path
        .segments
        .last()
        .map_or(false, |seg| seg.ident == "Session")
}

#[proc_macro_attribute]
pub fn function(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    quote!(#func).into()
}

#[proc_macro_attribute]
pub fn service(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut imp = parse_macro_input!(item as syn::ItemImpl);

    let attr_ts: TokenStream2 = attr.into();
    let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
    let args: AttrArgs = parser.parse2(attr_ts).expect("failed to parse attributes");
    let _service_name = extract_optional(&args, "name"); // TODO use later

    let mut generated = vec![];

    for item in imp.items.iter() {
        let method = match item {
            syn::ImplItem::Fn(m) => m,
            _ => continue,
        };

        // iterate attributes on each method
        for attr in &method.attrs {
            if !attr.path().is_ident("function") {
                continue;
            }

            let metas = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated);

            match metas {
                Ok(metas) => {
                    let id = extract(&metas, "id");
                    let description = extract_optional(&metas, "description");
                    let method_ident = method.sig.ident.clone();

                    let non_self_params: Vec<_> = method
                        .sig
                        .inputs
                        .iter()
                        .filter(|arg| !matches!(arg, syn::FnArg::Receiver(_)))
                        .collect();

                    let input_type = match non_self_params.first() {
                        Some(syn::FnArg::Typed(pat_type)) => {
                            let ty = &*pat_type.ty;
                            quote! { #ty }
                        }
                        _ => quote! { () },
                    };

                    let has_session_param =
                        if let Some(syn::FnArg::Typed(pat_type)) = non_self_params.get(1) {
                            if is_option_arc_session(&pat_type.ty) {
                                true
                            } else if type_contains_ident(&pat_type.ty, "Session") {
                                let actual = pat_type.ty.to_token_stream().to_string();
                                panic!(
                                    "Session parameter on `{}` must be typed as \
                                 `Option<Arc<Session>>`, found: `{}`",
                                    method_ident, actual
                                );
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                    // Extract return type
                    let return_type = match &method.sig.output {
                        syn::ReturnType::Type(_, ty) => &**ty,
                        syn::ReturnType::Default => {
                            panic!("Function {} must return FunctionResult", method_ident);
                        }
                    };

                    // Check if return type is FunctionResult<T, ErrorBody>
                    // If T is not Option<Value>, we need to serialize it to a JSON string
                    let needs_serialization = needs_serialization(return_type);
                    let handler_ident = format_ident!("{}_handler", method_ident);
                    let description = quote!(Some(#description.into()));

                    let method_call = if has_session_param {
                        quote! { this.#method_ident(input, session).await }
                    } else {
                        quote! { this.#method_ident(input).await }
                    };

                    let result_handling = if needs_serialization {
                        quote! {
                            let result = #method_call;
                            match result {
                                FunctionResult::Success(value) => {
                                    match serde_json::to_value(&value) {
                                        Ok(value) => FunctionResult::Success(Some(value)),
                                        Err(err) => {
                                            eprintln!(
                                                "[warning] Failed to serialize result for {}: {}",
                                                #id,
                                                err
                                            );
                                            FunctionResult::Failure(ErrorBody {
                                                code: "serialization_error".into(),
                                                message: format!("Failed to serialize result for {}: {}", #id, err.to_string()),
                                                stacktrace: None,
                                            })
                                        }
                                    }
                                }
                                FunctionResult::Failure(err) => FunctionResult::Failure(err),
                                FunctionResult::Deferred => FunctionResult::Deferred,
                                FunctionResult::NoResult => FunctionResult::NoResult,
                            }
                        }
                    } else {
                        method_call
                    };

                    // Generate request_format schema
                    let input_type_str = input_type.to_string();
                    let request_format_expr = if input_type_str == "()" {
                        quote! { None }
                    } else {
                        quote! {
                            Some(
                                serde_json::to_value(
                                    schemars::schema_for!(#input_type)
                                ).unwrap()
                            )
                        }
                    };

                    // Generate response_format schema
                    let response_format_expr = match extract_success_type(return_type) {
                        Some(success_ty) => {
                            quote! {
                                Some(
                                    serde_json::to_value(
                                        schemars::schema_for!(#success_ty)
                                    ).unwrap()
                                )
                            }
                        }
                        None => quote! { None },
                    };

                    let handler_and_registration = if has_session_param {
                        quote! {
                            let this = self.clone();
                            let #handler_ident = SessionHandler::new(move |input: Value, session: Option<::std::sync::Arc<Session>>| {
                                let this = this.clone();

                                async move {
                                    let parsed: Result<#input_type, _> = serde_json::from_value(input);
                                    let input = match parsed {
                                        Ok(v) => v,
                                        Err(err) => {
                                            eprintln!(
                                                "[warning] Failed to deserialize input for {}: {}",
                                                #id,
                                                err
                                            );
                                            return FunctionResult::Failure(ErrorBody {
                                                code: "deserialization_error".into(),
                                                message: format!("Failed to deserialize input for {}: {}", #id, err.to_string()),
                                                stacktrace: None,
                                            });
                                        }
                                    };

                                    #result_handling
                                }
                            });

                            engine.register_function_handler_with_session(
                                RegisterFunctionRequest {
                                    function_id: #id.into(),
                                    description: #description,
                                    request_format: #request_format_expr,
                                    response_format: #response_format_expr,
                                    metadata: None,
                                },
                                #handler_ident,
                            );
                        }
                    } else {
                        quote! {
                            let this = self.clone();
                            let #handler_ident = Handler::new(move |input: Value| {
                                let this = this.clone();

                                async move {
                                    let parsed: Result<#input_type, _> = serde_json::from_value(input);
                                    let input = match parsed {
                                        Ok(v) => v,
                                        Err(err) => {
                                            eprintln!(
                                                "[warning] Failed to deserialize input for {}: {}",
                                                #id,
                                                err
                                            );
                                            return FunctionResult::Failure(ErrorBody {
                                                code: "deserialization_error".into(),
                                                message: format!("Failed to deserialize input for {}: {}", #id, err.to_string()),
                                                stacktrace: None,
                                            });
                                        }
                                    };

                                    #result_handling
                                }
                            });

                            engine.register_function_handler(
                                RegisterFunctionRequest {
                                    function_id: #id.into(),
                                    description: #description,
                                    request_format: #request_format_expr,
                                    response_format: #response_format_expr,
                                    metadata: None,
                                },
                                #handler_ident,
                            );
                        }
                    };

                    generated.push(quote! {
                        {
                            #handler_and_registration
                        }
                    });
                }
                Err(e) => panic!("failed to parse attributes: {}", e),
            }
        }
    }

    let register_fn = quote! {
        fn register_functions(&self, engine: ::std::sync::Arc<Engine>) {
            #(#generated)*
        }
    };

    imp.items.push(syn::ImplItem::Verbatim(register_fn));

    quote!(#imp).into()
}
