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
                    let name = extract(&metas, "name");
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

                    let handler_ident = format_ident!("{}_handler", method_ident);
                    let description = quote!(Some(#description.into()));

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
                                                #name,
                                                err
                                            );
                                            return FunctionResult::Failure(ErrorBody {
                                                code: "deserialization_error".into(),
                                                message: format!("Failed to deserialize input for {}: {}", #name, err.to_string()),
                                            });
                                        }
                                    };

                                    this.#method_ident(input).await
                                }
                            });

                            engine.register_function_handler(
                                RegisterFunctionRequest {
                                    function_path: #name.into(),
                                    description: #description,
                                    request_format: None,
                                    response_format: None,
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
