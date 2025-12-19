// SPDX-License-Identifier: AGPL-3.0-only
use crate::service::process::HttpMethod;
use crate::service::route::RouteArgs;
use manyhow::{Emitter, ErrorMessage};
use proc_macro2::{Delimiter, Ident, Literal, TokenStream, TokenTree};
use quote::quote;
use std::collections::VecDeque;
use syn::LitStr;

pub struct MacroProcessResult {
    pub pre_fn_result: Vec<TokenTree>,
    pub route_args: RouteArgs,
    pub example: Option<LitStr>,
    pub doc: Vec<Literal>,
}

pub fn process(
    emitter: &mut Emitter,
    crate_prefix: &TokenStream,
    trait_ident: &Ident,
    fn_ident: &Ident,
    mut pre_fn: VecDeque<TokenTree>,
    errors: &mut TokenStream,
    post_fn: &mut Vec<TokenTree>,
    result: &mut Vec<TokenTree>,
) -> Result<MacroProcessResult, ()> {
    let mut route_args = None;
    let mut pre_fn_result0 = VecDeque::new();
    let mut pre_fn_result1 = Vec::new();
    let mut description = Vec::new();
    let mut example = None;

    let result_edited = if result.is_empty() {
        post_fn.extend(quote! { -> });
        *result = quote! { std::result::Result<(), _> }.into_iter().collect();
        true
    } else {
        false
    };

    while let Some(pre_fn_tree) = pre_fn.pop_front() {
        let TokenTree::Punct(punct) = pre_fn_tree.clone() else {
            pre_fn_result0.push_back(pre_fn_tree);
            continue;
        };
        if punct.as_char() != '#' {
            pre_fn_result0.push_back(pre_fn_tree);
            continue;
        }
        let Some(TokenTree::Group(group)) = pre_fn.pop_front() else {
            pre_fn_result0.push_back(pre_fn_tree);
            continue;
        };
        if group.delimiter() != Delimiter::Bracket {
            pre_fn_result0.push_back(pre_fn_tree);
            pre_fn_result0.push_back(TokenTree::Group(group));
            continue;
        }
        let mut macro_inner: VecDeque<_> = group.stream().into_iter().collect();
        let Some(TokenTree::Ident(ident)) = macro_inner.pop_front() else {
            pre_fn_result0.push_back(pre_fn_tree);
            pre_fn_result0.push_back(TokenTree::Group(group));
            continue;
        };
        match ident.to_string().as_str() {
            "route" => {
                let arg = macro_inner.pop_front();
                let Some(TokenTree::Group(group)) = arg else {
                    emitter.emit(ErrorMessage::new(
                        arg.unwrap_or(TokenTree::Ident(ident)).span(),
                        "Expected arguments for `route` attribute",
                    ));
                    return Err(());
                };
                if group.delimiter() != Delimiter::Parenthesis {
                    emitter.emit(ErrorMessage::new(
                        group.span(),
                        "Expected arguments for `route` attribute, in parenthesis",
                    ));
                    return Err(());
                }
                match syn::parse2::<RouteArgs>(group.stream()) {
                    Ok(args) => {
                        route_args = Some(args);
                    }
                    Err(err) => panic!("Failed to parse route macro arguments: {err}"),
                }
            }
            "doc" => {
                if let Some(TokenTree::Punct(punct)) = macro_inner.pop_front()
                    && punct.as_char() == '='
                    && let Some(TokenTree::Literal(lit)) = macro_inner.pop_front()
                {
                    description.push(lit);
                }
                pre_fn_result0.push_back(pre_fn_tree);
                pre_fn_result0.push_back(TokenTree::Group(group));
            }
            "example" => {
                let arg = macro_inner.pop_front();
                let Some(TokenTree::Group(group)) = arg else {
                    emitter.emit(ErrorMessage::new(
                        arg.unwrap_or(TokenTree::Ident(ident)).span(),
                        "Expected arguments for `example` attribute",
                    ));
                    return Err(());
                };
                if group.delimiter() != Delimiter::Parenthesis {
                    emitter.emit(ErrorMessage::new(
                        group.span(),
                        "Expected arguments for `example` attribute, in parenthesis",
                    ));
                    return Err(());
                }
                example = Some(
                    syn::parse2(group.stream()).expect("Expected literal string as example path"),
                );
            }
            _ => {
                pre_fn_result0.push_back(pre_fn_tree);
                pre_fn_result0.push_back(TokenTree::Group(group));
            }
        }
    }

    let route_args = route_args.take().expect("Expected route attribute");

    if HttpMethod::Ws == route_args.method && result_edited {
        *result = quote! { std::result::Result<#crate_prefix WebsocketResponse, _> }
            .into_iter()
            .collect();
    }

    while let Some(pre_fn_tree) = pre_fn_result0.pop_front() {
        let TokenTree::Punct(punct) = pre_fn_tree.clone() else {
            pre_fn_result1.push(pre_fn_tree);
            continue;
        };
        if punct.as_char() != '#' {
            pre_fn_result1.push(pre_fn_tree);
            continue;
        }
        let Some(TokenTree::Group(group)) = pre_fn_result0.pop_front() else {
            pre_fn_result1.push(pre_fn_tree);
            continue;
        };
        if group.delimiter() != Delimiter::Bracket {
            pre_fn_result1.push(pre_fn_tree);
            pre_fn_result1.push(TokenTree::Group(group));
            continue;
        }
        let mut macro_inner: VecDeque<_> = group.stream().into_iter().collect();
        let Some(TokenTree::Ident(ident)) = macro_inner.pop_front() else {
            pre_fn_result1.push(pre_fn_tree);
            pre_fn_result1.push(TokenTree::Group(group));
            continue;
        };

        let fn_ident = fn_ident.clone();
        match ident.to_string().as_str() {
            #[cfg(not(feature = "error"))]
            "error" => {
                emitter.emit(ErrorMessage::new(
                    ident.span(),
                    "The error feature has been disabled",
                ));
                return Err(());
            }
            #[cfg(feature = "error")]
            "error" => {
                let arg = macro_inner.pop_front();
                let Some(TokenTree::Group(group)) = arg else {
                    emitter.emit(ErrorMessage::new(
                        arg.unwrap_or(TokenTree::Ident(ident)).span(),
                        "Expected arguments for `error` attribute",
                    ));
                    return Err(());
                };
                if group.delimiter() != Delimiter::Parenthesis {
                    emitter.emit(ErrorMessage::new(
                        group.span(),
                        "Expected arguments for `error` attribute, in parenthesis",
                    ));
                    return Err(());
                }

                let mut args = TokenStream::new();
                if !route_args.no_auth {
                    args.extend(quote! { , auth::invalid });
                }
                if !group.stream().is_empty() {
                    args.extend(quote! { , });
                    args.extend(group.stream());
                }
                errors.extend(quote! {
                        #crate_prefix error::error!([< #fn_ident:camel Error >] #args);
                });

                *result = result
                    .clone()
                    .into_iter()
                    .flat_map(|tree| {
                        if let TokenTree::Ident(ident) = tree {
                            if ident.to_string().eq("_") {
                                quote! {
                                    #crate_prefix paste! { [< #trait_ident:snake _error >]::[< #fn_ident:camel Error >] }
                                }
                                    .into_iter()
                                    .collect()
                            } else {
                                vec![TokenTree::Ident(ident)]
                            }
                        } else {
                            vec![tree]
                        }
                    })
                    .collect();
            }
            _ => {
                pre_fn_result1.push(pre_fn_tree);
                pre_fn_result1.push(TokenTree::Group(group));
            }
        }
    }

    Ok(MacroProcessResult {
        pre_fn_result: pre_fn_result1,
        route_args,
        example,
        doc: description,
    })
}
