use crate::SUCCESS_DESCRIPTION;
use crate::responses::DocumentedResultResponse;
use crate::utils::MaybeUnit;
use std::collections::HashMap;
use utoipa::ToSchema;
use utoipa::openapi::{Content, RefOr, Response, ResponsesBuilder, Schema};

impl<V: ToSchema + MaybeUnit + 'static> DocumentedResultResponse for V {
    fn openapi(
        responses: ResponsesBuilder,
        schemas: &mut HashMap<String, RefOr<Schema>>,
    ) -> ResponsesBuilder {
        if V::unit().is_some() {
            responses.response(
                "204",
                Response::builder().description(SUCCESS_DESCRIPTION).build(),
            )
        } else {
            let response_schema = V::schema();
            let mut vals = Vec::new();
            if matches!(response_schema, RefOr::Ref(_)) {
                vals.push((V::name().to_string(), V::schema()));
            }
            V::schemas(&mut vals);
            schemas.extend(vals);
            responses.response(
                "200",
                Response::builder()
                    .description(SUCCESS_DESCRIPTION)
                    .content("application/json", Content::new(Some(response_schema)))
                    .build(),
            )
        }
    }
}
