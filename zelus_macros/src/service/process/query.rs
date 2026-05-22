// SPDX-License-Identifier: AGPL-3.0-only
use crate::service::parse::FunctionArgument;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens as _, quote};
use syn::LitStr;

pub fn process(
    crate_prefix: &TokenStream,
    trait_ident: &Ident,
    fn_ident: &Ident,
    fn_args_identified: &[FunctionArgument],
    routes: &mut TokenStream,
    http_args: &mut TokenStream,
    func_args: &mut Vec<TokenStream>,
    operations: &mut TokenStream,
    schema_extra: &mut TokenStream,
    client_def_body: &mut TokenStream,
    fn_def_args: &mut TokenStream,
    fn_def_call: &mut TokenStream,
) {
    let fn_args_query: Vec<_> = fn_args_identified
        .iter()
        .filter_map(|arg| {
            if let FunctionArgument::Query {
                variable_name,
                variable_type_wopt,
                required,
                desc,
                no_schema,
                ..
            } = arg
            {
                Some((
                    variable_name.clone(),
                    variable_type_wopt.clone(),
                    *required,
                    desc.clone(),
                    *no_schema,
                ))
            } else {
                None
            }
        })
        .collect();
    if !fn_args_query.is_empty() {
        let mut client_def_body_pre = TokenStream::new();
        let mut client_def_body_post = quote! {
            zelus_result_url.query_pairs_mut()
        };

        let mut query_fields = TokenStream::new();
        let mut query_names = TokenStream::new();

        let mut query_schema_fields = TokenStream::new();

        for (arg_name, arg_type, required, desc, no_schema) in fn_args_query {
            query_fields.extend(quote! {
                #arg_name: #arg_type,
            });
            query_names.extend(quote! {
                #arg_name,
            });

            let variable_literal = LitStr::new(&arg_name.to_string(), arg_name.span());
            let required_indent =
                Ident::new(if required { "True" } else { "False" }, Span::call_site());

            let desc = desc.map_or_else(
                || quote! { [< variable_query_ #arg_name:snake >]::DESCRIPTION },
                LitStr::into_token_stream,
            );
            let schema_if = if no_schema {
                TokenStream::new()
            } else {
                query_schema_fields.extend(quote! {
                    #arg_name: #arg_type,
                });
                let arg_name_lit = LitStr::new(&arg_name.to_string(), arg_name.span());

                quote! {
                    .schema(Some(
                        {
                            let #crate_prefix utoipa::openapi::Schema::Object(obj) = #crate_prefix utoipa::openapi::schema::Schema::from(< [< __ #trait_ident:snake _zelus_routes >]::[< #fn_ident:camel QuerySchema >] as #crate_prefix utoipa::PartialSchema >::schema())
                                else {
                                 unreachable!();
                            };
                            obj.properties.get(#arg_name_lit).expect("schema not generated").clone()
                        }
                    ))
                }
            };
            operations.extend(quote! {
                operations = operations.parameter(
                    #crate_prefix utoipa::openapi::path::ParameterBuilder::from(
                        #crate_prefix utoipa::openapi::path::Parameter::new(#variable_literal)
                    )
                    .parameter_in(#crate_prefix utoipa::openapi::path::ParameterIn::Query)
                    .description(Some(#desc))
                    .required(#crate_prefix utoipa::openapi::Required::#required_indent)
                    #schema_if,
                );
            });

            fn_def_args.extend(quote! { #arg_name: #arg_type, });
            fn_def_call.extend(quote! { #arg_name, });

            client_def_body_pre.extend(quote! {
                let #arg_name: Option<String> = #arg_name.serialize(#crate_prefix internal::StringSerializer).unwrap();
            });
            client_def_body_post.extend(quote! {
                .extend_pairs(#arg_name.map(|val| (#variable_literal, val)).iter())
            });
        }

        client_def_body_post.extend(quote! { ; });

        client_def_body.extend(client_def_body_pre);
        client_def_body.extend(client_def_body_post);

        routes.extend(quote! {
            #[derive(Debug, Clone, #crate_prefix serde::Serialize, #crate_prefix serde::Deserialize)]
            pub(crate) struct [< #fn_ident:camel Query >] {
                #query_fields
            }

            #[derive(#crate_prefix utoipa::ToSchema)]
            pub(crate) struct [< #fn_ident:camel QuerySchema >] {
                #query_schema_fields
            }
        });

        schema_extra.extend(quote! {
            let mut schemas_vec = Vec::new();
            < [< __ #trait_ident:snake _zelus_routes >]::[< #fn_ident:camel QuerySchema >] as #crate_prefix utoipa::ToSchema >::schemas(&mut schemas_vec);
            schemas.extend(schemas_vec);
        });

        http_args.extend(quote! {
                #crate_prefix axum::extract::Query(
                    [< #fn_ident:camel Query >] { #query_names }
                ):
                #crate_prefix axum::extract::Query<
                    [< #fn_ident:camel Query >]
                >,
        });
        func_args
            .push(quote! { #crate_prefix axum::extract::Query< [< #fn_ident:camel Query >] > });
    }
}
