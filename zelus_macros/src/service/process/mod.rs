// SPDX-License-Identifier: AGPL-3.0-only
mod header;
mod macros;
mod path;
mod payload;
mod query;
mod route;
mod special;

use crate::service::args::ServiceArgs;
use crate::service::parse::FunctionArgument;
use crate::service::process::macros::MacroProcessResult;
use crate::service::route::RouteArgs;
use crate::service::utils::{
    TokenStreamArray, attribute_handle, parse_function_argument, type_option,
};
use core::fmt::{Display, Formatter};
use core::str::FromStr;
use either::Either;
use itertools::Itertools;
use lazy_regex::regex_replace_all;
use manyhow::{Emitter, ErrorMessage};
use proc_macro2::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};
use quote::quote;
use std::collections::{HashMap, VecDeque};
use syn::LitStr;

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "common http methods order"
)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
    Trace,
    Ws,
}

impl Display for HttpMethod {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::Get | Self::Ws => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Trace => "TRACE",
        })
    }
}

impl FromStr for HttpMethod {
    type Err = ();

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        match str.to_lowercase().as_str() {
            "get" => Ok(Self::Get),
            "post" => Ok(Self::Post),
            "put" => Ok(Self::Put),
            "patch" => Ok(Self::Patch),
            "delete" => Ok(Self::Delete),
            "head" => Ok(Self::Head),
            "options" => Ok(Self::Options),
            "trace" => Ok(Self::Trace),
            "ws" => Ok(Self::Ws),
            _ => Err(()),
        }
    }
}

impl HttpMethod {
    pub(crate) fn to_http(self) -> Ident {
        Ident::new(&self.to_string(), Span::call_site())
    }

    pub(crate) fn to_utoipa(self) -> Ident {
        Ident::new(
            match self {
                Self::Get | Self::Ws => "Get",
                Self::Post => "Post",
                Self::Put => "Put",
                Self::Patch => "Patch",
                Self::Delete => "Delete",
                Self::Head => "Head",
                Self::Options => "Options",
                Self::Trace => "Trace",
            },
            Span::call_site(),
        )
    }
}

pub struct ProcessedFunction {
    client_def: TokenStream,
    client_impl: TokenStream,

    fn_ident: Ident,
    method: HttpMethod,
    operations: TokenStream,
    path: String,
    result: TokenStream,
    schema_extra: TokenStream,
    routes_selection: Vec<Ident>,
    fn_args_impl: Group,
    function_impl: Option<Group>,
}

pub fn process(
    emitter: &mut Emitter,
    crate_prefix: &TokenStream,
    args: &ServiceArgs,

    trait_ident: &Ident,
    fn_keyword: &Ident,
    fn_ident: Ident,
    end: Either<Punct, Group>,

    pre_fn: VecDeque<TokenTree>,
    mut post_fn: Vec<TokenTree>,

    mut fn_args: Group,
    errors: &mut TokenStream,
    routes: &mut TokenStream,
    mut result: Vec<TokenTree>,
    functions: &mut Vec<ProcessedFunction>,
) -> Result<Vec<TokenTree>, ()> {
    let mut operations = TokenStream::new();

    if let Some(tag) = &args.tag {
        operations.extend(quote! { operations = operations.tag(#tag); });
    }

    let MacroProcessResult {
        pre_fn_result,
        route_args:
            RouteArgs {
                absolute,
                path,
                path_span,
                method,
                query,
                routes: routes_selection,
                raw,
                ..
            },
        example,
        doc,
    } = macros::process(
        emitter,
        crate_prefix,
        trait_ident,
        &fn_ident,
        pre_fn,
        errors,
        &mut post_fn,
        &mut result,
    )?;

    if let Some(summary) = doc.first() {
        operations.extend(quote! { operations = operations.summary(Some(#summary)); });

        let desc: TokenStream = doc
            .into_iter()
            .intersperse(Literal::string("<br/>\n"))
            .map(|literal| {
                let Ok(str) = syn::parse2::<LitStr>(
                    core::iter::once(TokenTree::Literal(literal.clone())).collect(),
                ) else {
                    return literal;
                };
                if str.value().chars().all(|ch| ch == ' ') {
                    Literal::string("")
                } else {
                    Literal::string(&regex_replace_all!(
                        "\\[(\\`[A-Za-z0-9:\\(\\)\\{\\}\\.;_-]+\\`)\\]",
                        &str.value(),
                        |_, inner: &str| inner.to_owned()
                    ))
                }
            })
            .map(TokenTree::Literal)
            .intersperse(TokenTree::Punct(Punct::new(',', Spacing::Joint)))
            .collect();

        operations.extend(quote! { operations = operations.description(Some(concat!(#desc))); });
    }

    let path = if absolute {
        path
    } else {
        format!("{}{}", args.path, path)
    };

    let mut fn_args_identified = Vec::new();

    let fn_args_array: TokenStreamArray =
        syn::parse2(fn_args.stream()).expect("Expected function arguments");

    let mut fn_args_out = TokenStream::new();
    let mut fn_args_impl_out = TokenStream::new();

    for (fn_index, fn_arg) in fn_args_array.0.into_iter().enumerate() {
        let fn_arg_unmodified = fn_arg.clone();
        if fn_index == 0 {
            let mut fn_arg = fn_arg.into_iter();
            let Some(TokenTree::Punct(punct)) = fn_arg.next() else {
                emitter.emit(ErrorMessage::new(
                    proc_macro::TokenStream::from(fn_arg_unmodified),
                    "Your function needs to have &self as the first argument",
                ));
                return Err(());
            };
            if punct.as_char() != '&' {
                emitter.emit(ErrorMessage::new(
                    proc_macro::TokenStream::from(fn_arg_unmodified),
                    "Your function needs to have &self as the first argument",
                ));
                return Err(());
            }
            let Some(TokenTree::Ident(ident)) = fn_arg.next() else {
                emitter.emit(ErrorMessage::new(
                    proc_macro::TokenStream::from(fn_arg_unmodified),
                    "Your function needs to have &self as the first argument",
                ));
                return Err(());
            };
            if !ident.to_string().eq("self") {
                emitter.emit(ErrorMessage::new(
                    proc_macro::TokenStream::from(fn_arg_unmodified),
                    "Your function needs to have &self as the first argument",
                ));
                return Err(());
            }

            fn_args_out.extend(quote! { #fn_arg_unmodified, });
            fn_args_impl_out.extend(quote! { #fn_arg_unmodified, });
            if HttpMethod::Ws == method {
                fn_args_out
                    .extend(quote! { ws: #crate_prefix axum::extract::ws::WebSocketUpgrade, });
                fn_args_impl_out
                    .extend(quote! { ws: #crate_prefix axum::extract::ws::WebSocketUpgrade, });
            }

            continue;
        }

        let mut fn_arg_out_stripped: TokenStream = fn_arg_unmodified.clone();
        let attributes =
            attribute_handle(emitter, ["special", "no_schema"], &mut fn_arg_out_stripped)?;
        let special = attributes[0];
        let no_schema = attributes[1];

        let (fn_arg_name, fn_arg_type, trait_arg_out) =
            parse_function_argument(emitter, fn_arg_out_stripped.clone(), fn_index)?;

        fn_args_out.extend(quote! { #trait_arg_out, });
        fn_args_impl_out.extend(quote! { #fn_arg_out_stripped, });

        let (fn_arg_type_opt, fn_arg_type_opt_used) = type_option(fn_arg_type.clone());

        if special {
            fn_args_identified.push(FunctionArgument::Special {
                variable_name: fn_arg_name,
                variable_type: fn_arg_type,
            });
        } else if path.contains(&format!("{{{fn_arg_name}}}")) {
            fn_args_identified.push(FunctionArgument::Path {
                variable_name: fn_arg_name,
                variable_type: fn_arg_type,
                no_schema,
            });
        } else if let Some(desc) = query.get(&fn_arg_name.to_string()) {
            fn_args_identified.push(FunctionArgument::Query {
                variable_name: fn_arg_name,
                variable_type_wopt: fn_arg_type,
                required: !fn_arg_type_opt_used,
                desc: desc.clone(),
                no_schema,
            });
        } else if fn_arg_type_opt
            .clone()
            .into_iter()
            .next()
            .is_some_and(|token| {
                if let TokenTree::Ident(i) = token {
                    i.to_string().eq_ignore_ascii_case("TypedHeader")
                } else {
                    false
                }
            })
        {
            fn_args_identified.push(FunctionArgument::Header {
                variable_name: fn_arg_name,
                variable_type: fn_arg_type_opt,
                variable_type_wopt: fn_arg_type,
                required: !fn_arg_type_opt_used,
            });
        } else {
            fn_args_identified.push(FunctionArgument::Payload {
                variable_name: fn_arg_name,
                variable_type: fn_arg_type,
            });
        }
    }

    let mut group = Group::new(fn_args.delimiter(), fn_args_out);
    group.set_span(fn_args.span());
    fn_args = group;

    let mut fn_args_impl = Group::new(fn_args.delimiter(), fn_args_impl_out);
    fn_args_impl.set_span(fn_args.span());

    let mut func_args = Vec::new();

    let mut http_args = TokenStream::new();
    let call_args: TokenStream = fn_args_identified
        .iter()
        .flat_map(|argument| {
            let (FunctionArgument::Path { variable_name, .. }
            | FunctionArgument::Query { variable_name, .. }
            | FunctionArgument::Header { variable_name, .. }
            | FunctionArgument::Payload { variable_name, .. }
            | FunctionArgument::Special { variable_name, .. }) = argument;
            quote! { #variable_name, }
        })
        .collect();

    let mut schema_extra = TokenStream::new();

    let mut client_impl_body = TokenStream::new();
    let mut client_def_body = TokenStream::new();
    let mut fn_def_args = quote! { &self, };
    let mut fn_def_call = TokenStream::new();

    path::process(
        emitter,
        crate_prefix,
        trait_ident,
        &fn_ident,
        (&path, path_span),
        &fn_args_identified,
        routes,
        &mut http_args,
        &mut func_args,
        &mut operations,
        &mut schema_extra,
        &mut client_def_body,
        &mut fn_def_args,
        &mut fn_def_call,
    )?;

    query::process(
        crate_prefix,
        &fn_ident,
        &fn_args_identified,
        routes,
        &mut http_args,
        &mut func_args,
        &mut operations,
        &mut schema_extra,
        &mut client_def_body,
        &mut fn_def_args,
        &mut fn_def_call,
    );

    header::process(
        crate_prefix,
        &fn_args_identified,
        &mut http_args,
        &mut func_args,
        &mut operations,
        &mut client_impl_body,
    );

    special::process(
        crate_prefix,
        &fn_args_identified,
        &result,
        &mut http_args,
        &mut func_args,
        &mut operations,
        &mut client_impl_body,
    );

    payload::process(
        emitter,
        crate_prefix,
        trait_ident,
        &fn_ident,
        &fn_args_identified,
        routes,
        &mut http_args,
        &mut func_args,
        &mut operations,
        &mut schema_extra,
        example,
        &mut client_impl_body,
    )?;

    route::process(
        crate_prefix,
        method,
        raw,
        trait_ident,
        &fn_ident,
        func_args,
        http_args,
        call_args,
        routes,
    );

    let mut func = Vec::new();
    func.extend(pre_fn_result);
    func.push(TokenTree::Ident(fn_keyword.clone()));
    func.push(TokenTree::Ident(fn_ident.clone()));
    func.push(TokenTree::Group(fn_args.clone()));
    func.append(&mut post_fn.clone());
    func.append(&mut result.clone());
    func.push(TokenTree::Punct(match &end {
        Either::Left(end) => end.clone(),
        Either::Right(_) => Punct::new(';', Spacing::Joint),
    }));

    let post_fn = TokenStream::from_iter(post_fn);
    let result = TokenStream::from_iter(result);
    let fn_def_args = Group::new(Delimiter::Parenthesis, fn_def_args);
    let fn_args = TokenTree::Group(fn_args);

    let fn_def_call = Group::new(Delimiter::Parenthesis, fn_def_call);
    let method_http = method.to_http();
    if method == HttpMethod::Ws {
        client_impl_body =
            quote! { panic!("Websocket client is not implemented (and will maybe never be)"); };
    } else if raw {
        client_impl_body = quote! {
            let mut request = self.client().request(#crate_prefix http::Method::#method_http, self.[< #fn_ident _url >] #fn_def_call);
            #client_impl_body
            let response = request
                .send()
                .await?;
            match response.error_for_status_ref() {
                Ok(_) => {
                    Ok(#crate_prefix internal::from_raw(response))
                },
                Err(_err) => {
                    Err(#crate_prefix internal::error_by_response(response).await)
                }
            }
        };
    } else {
        client_impl_body = quote! {
            let mut request = self.client().request(#crate_prefix http::Method::#method_http, self.[< #fn_ident _url >] #fn_def_call)
                .header(#crate_prefix http::header::ACCEPT, "application/json");
            #client_impl_body
            let response = request
                .send()
                .await?;
            match response.error_for_status_ref() {
                Ok(_) => {
                    Ok(#crate_prefix internal::FrameworkJsonResponse::from_reqwest(response)
                        .await?.0)
                },
                Err(_err) => {
                    Err(#crate_prefix internal::error_by_response(response).await)
                }
            }
        };
    }

    let client_impl = quote! { async #fn_keyword #fn_ident #fn_args #post_fn #result {
        #client_impl_body
    } };

    let client_def = quote! {

        #fn_keyword [< #fn_ident _url >] #fn_def_args -> #crate_prefix url::Url {
            #client_def_body
            zelus_result_url
        }

    };

    functions.push(ProcessedFunction {
        client_def,
        client_impl,
        fn_ident,
        method,
        operations,
        path,
        result,
        schema_extra,
        routes_selection,
        fn_args_impl,
        function_impl: end.right(),
    });

    Ok(func)
}

pub fn finish(
    crate_prefix: &TokenStream,
    args: &ServiceArgs,
    trait_ident: &Ident,
    trait_body_output: &mut Vec<TokenTree>,
    output_extra: &mut Vec<TokenTree>,
    functions: Vec<ProcessedFunction>,
    impl_data: Option<(Ident, Ident, Ident)>,
) {
    let mut routes = HashMap::new();

    let mut client_impl_merged = TokenStream::new();
    let mut client_def_merged = TokenStream::new();
    let mut impl_tokens = TokenStream::new();

    for ProcessedFunction {
        fn_ident,
        path,
        method,
        routes_selection,
        result,
        operations,
        schema_extra,
        client_impl,
        client_def,
        fn_args_impl,
        function_impl,
        ..
    } in functions
    {
        client_impl_merged.extend(client_impl);
        client_def_merged.extend(client_def);

        let method = method.to_utoipa();

        let path = LitStr::new(&path, Span::call_site());

        let operation_id = LitStr::new(&fn_ident.to_string(), Span::call_site());

        let token = quote! {

            .route(
                #path,
                #crate_prefix utoipa::openapi::HttpMethod::#method,
                {
                    let mut schemas = std::collections::HashMap::new();
                    #schema_extra

                    let responses = <#result as #crate_prefix responses::DocumentedResponse>::openapi(
                        #crate_prefix utoipa::openapi::ResponsesBuilder::new(),
                        &mut schemas
                    );
                    let mut operations = OperationBuilder::new()
                        .operation_id(Some(#operation_id));
                    #operations

                    (responses, operations, schemas.into_iter().collect())
                },
                MethodRouter::new()
                    .on(#crate_prefix utoipa::openapi::HttpMethod::#method.to_method_filter(), [< __ #trait_ident:snake _zelus_routes >]::#fn_ident(self.clone())),
            )
        };

        for selection in routes_selection {
            routes
                .entry(selection.to_string())
                .or_insert_with(|| (selection, TokenStream::new()))
                .1
                .extend(token.clone());
        }

        if let Some(function_impl) = function_impl {
            impl_tokens.extend(quote! {
                async fn #fn_ident #fn_args_impl -> #result #function_impl
            });
        }
    }

    routes
        .entry("default".to_owned())
        .or_insert_with(|| (Ident::new("default", Span::call_site()), TokenStream::new()));
    routes.entry("with_auth".to_owned()).or_insert_with(|| {
        (
            Ident::new("with_auth", Span::call_site()),
            TokenStream::new(),
        )
    });
    routes.entry("without_auth".to_owned()).or_insert_with(|| {
        (
            Ident::new("without_auth", Span::call_site()),
            TokenStream::new(),
        )
    });

    for (selection, routes) in routes.into_values() {
        trait_body_output.extend(quote! {

            #crate_prefix paste! {

                fn [< routes_ #selection >] (&self) -> #crate_prefix router::ZelusRouter where Self: Clone + Send + Sync + Sized + 'static {
                    use #crate_prefix router::ZelusRouter;
                    use #crate_prefix utoipa::openapi::path::OperationBuilder;
                    use #crate_prefix utoipa_axum::PathItemExt;
                    use #crate_prefix axum::routing::method_routing::MethodRouter;

                    let mut router = ZelusRouter::new();
                    let _ = (&mut router)
                        #routes;
                    router
                }
            }
        });
    }

    if let Some((impl_ident, for_ident, struct_ident)) = impl_data {
        output_extra.extend(quote! {
            #crate_prefix paste! {

                #[#crate_prefix async_trait]
                #impl_ident #trait_ident #for_ident #struct_ident {
                    #impl_tokens
                }

            }
        });
    }

    if !args.no_sdk {
        output_extra.extend(quote! {

        #crate_prefix paste! {

                pub trait [< #trait_ident URL >]: #crate_prefix sdk::ZelusClientImpl {
                    #client_def_merged
                }

                pub trait [< #trait_ident ClientImpl >]: #crate_prefix sdk::ZelusClientImpl {}

                impl<T: #trait_ident + #crate_prefix sdk::ZelusClientImpl> [< #trait_ident URL >] for T {}

                #[#crate_prefix async_trait]
                #[diagnostic::do_not_recommend]
                #[allow(unused_variables)] // TODO: Why is this needed?
                impl<T: [< #trait_ident ClientImpl >] + Send + Sync> #trait_ident for T {
                    #client_impl_merged
                }

            }

        });
    }
}
