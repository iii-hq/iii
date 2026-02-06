// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{ItemFn, Meta, Token, parse::Parser, parse_macro_input, punctuated::Punctuated};

type AttrArgs = Punctuated<Meta, Token![,]>;

fn extract(args: &Punctuated<Meta, Token![,]>, name: &str) -> String {
    for meta in args {
        if let Meta::NameValue(nv) = meta {
            if nv.path.is_ident(name) {
                if let syn::Expr::Lit(expr_lit) = &nv.value {
                    if let syn::Lit::Str(s) = &expr_lit.lit {
                        return s.value();
                    }
                }
            }
        }
    }
    panic!("Missing required attribute: {} = \"...\"", name);
}

fn extract_optional(args: &Punctuated<Meta, Token![,]>, name: &str) -> Option<String> {
    for meta in args {
        if let Meta::NameValue(nv) = meta {
            if nv.path.is_ident(name) {
                if let syn::Expr::Lit(expr_lit) = &nv.value {
                    if let syn::Lit::Str(s) = &expr_lit.lit {
                        return Some(s.value());
                    }
                }
            }
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

                    let input_type = match method
                        .sig
                        .inputs
                        .iter()
                        .skip_while(|arg| matches!(arg, syn::FnArg::Receiver(_)))
                        .next()
                    {
                        Some(syn::FnArg::Typed(pat_type)) => {
                            let ty = &*pat_type.ty;
                            quote! { #ty }
                        }
                        _ => quote! { () },
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

                    let result_handling = if needs_serialization {
                        quote! {
                            let result = this.#method_ident(input).await;
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
                        quote! {
                            this.#method_ident(input).await
                        }
                    };

                    generated.push(quote! {
                        {
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
                                    request_format: None,
                                    response_format: None,
                                    metadata: None,
                                },
                                #handler_ident,
                            );
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
