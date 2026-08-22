use crate::SUCCESS_DESCRIPTION;
use crate::responses::DocumentedResultResponse;
use crate::types::DataStream;
use axum::response::IntoResponse;
use axum_extra::headers::{ContentLength, HeaderMapExt};
use futures_util::TryStreamExt as _;
use std::collections::HashMap;
use std::io;
use utoipa::openapi::{Content, RefOr, Response, ResponsesBuilder, Schema};

pub struct FileResponse(pub DataStream); // TODO: Add content type as parameter, when https://github.com/rust-lang/rust/issues/95174 is stable

impl From<reqwest::Response> for FileResponse {
    fn from(value: reqwest::Response) -> Self {
        Self(DataStream::by_stream(
            value.headers().typed_get::<ContentLength>(),
            value.bytes_stream().map_err(io::Error::other),
        ))
    }
}

impl IntoResponse for FileResponse {
    fn into_response(self) -> axum::response::Response {
        self.0.into_axum()
    }
}

impl DocumentedResultResponse for FileResponse {
    fn openapi(
        responses: ResponsesBuilder,
        _schemas: &mut HashMap<String, RefOr<Schema>>,
    ) -> ResponsesBuilder {
        responses.response(
            "200",
            Response::builder()
                .description(SUCCESS_DESCRIPTION)
                .content("application/octet-stream", Content::builder().build())
                .build(),
        )
    }
}
