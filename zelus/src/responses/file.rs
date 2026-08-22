use crate::SUCCESS_DESCRIPTION;
use crate::responses::DocumentedResultResponse;
use crate::types::DataStream;
use axum::response::IntoResponse;
use axum_extra::{
    headers::{ContentDisposition, ContentLength, ContentType, HeaderMapExt},
    response::Attachment,
};
use futures_util::TryStreamExt as _;
use http::HeaderMap;
use std::collections::HashMap;
use std::io;
use utoipa::openapi::{Content, RefOr, Response, ResponsesBuilder, Schema};

// TODO: Add content type as parameter, when https://github.com/rust-lang/rust/issues/95174 is stable
pub enum FileResponse {
    Header {
        stream: DataStream,
        disposition: Option<ContentDisposition>,
        r#type: Option<ContentType>,
    },
    Axum(Attachment<DataStream>),
}

impl From<reqwest::Response> for FileResponse {
    fn from(value: reqwest::Response) -> Self {
        Self::Header {
            disposition: value.headers().typed_get::<ContentDisposition>(),
            r#type: value.headers().typed_get::<ContentType>(),
            stream: DataStream::by_stream(
                value.headers().typed_get::<ContentLength>(),
                value.bytes_stream().map_err(io::Error::other),
            ),
        }
    }
}

impl IntoResponse for FileResponse {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Header {
                stream,
                disposition,
                r#type,
            } => {
                let mut header = HeaderMap::new();
                if let Some(disposition) = disposition {
                    header.typed_insert(disposition);
                }
                if let Some(r#type) = r#type {
                    header.typed_insert(r#type);
                }
                (header, stream.into_axum_body()).into_response()
            }
            Self::Axum(attachment) => attachment.into_response(),
        }
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
