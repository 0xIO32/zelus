use crate::SUCCESS_DESCRIPTION;
use crate::responses::DocumentedResultResponse;
use crate::types::DataStream;
use axum::response::IntoResponse;
use axum_extra::headers::{ContentDisposition, ContentLength, ContentType, HeaderMapExt};
use futures_util::TryStreamExt as _;
use http::HeaderMap;
use std::collections::HashMap;
use std::io;
use utoipa::openapi::{Content, RefOr, Response, ResponsesBuilder, Schema};

// TODO: Add content type as parameter, when https://github.com/rust-lang/rust/issues/95174 is stable
pub struct FileResponse(
    pub Option<ContentDisposition>,
    pub Option<ContentType>,
    pub DataStream,
);

impl From<reqwest::Response> for FileResponse {
    fn from(value: reqwest::Response) -> Self {
        Self(
            value.headers().typed_get::<ContentDisposition>(),
            value.headers().typed_get::<ContentType>(),
            DataStream::by_stream(
                value.headers().typed_get::<ContentLength>(),
                value.bytes_stream().map_err(io::Error::other),
            ),
        )
    }
}

impl IntoResponse for FileResponse {
    fn into_response(self) -> axum::response::Response {
        let mut header = HeaderMap::new();
        if let Some(disposition) = self.0 {
            header.typed_insert(disposition);
        }
        if let Some(r#type) = self.1 {
            header.typed_insert(r#type);
        }
        (header, self.2.into_axum_body()).into_response()
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
