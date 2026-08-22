// SPDX-License-Identifier: AGPL-3.0-only
use axum::body::BodyDataStream;
use axum::extract::{FromRequest, Request};
use axum::response::IntoResponse;
use axum_extra::headers::{ContentLength, HeaderMapExt};
use bytes::Bytes;
use core::pin::Pin;
use futures_util::Stream;
use http::HeaderMap;
use std::io;
use tokio::io::AsyncRead;
use tokio_util::io::{ReaderStream, StreamReader};

/// Universal type for data streams over the network.
///
/// It can be received using axum and be used in a request with reqwest.
pub enum DataStream {
    Axum(BodyDataStream, Option<ContentLength>),
    Read(
        Pin<Box<dyn AsyncRead + Send + 'static>>,
        Option<ContentLength>,
    ),
    Stream(Pin<BoxedStream>, Option<ContentLength>),
}

type BoxedStream = Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>;
impl DataStream {
    #[must_use]
    pub const fn length(&self) -> Option<ContentLength> {
        let (Self::Axum(_, length) | Self::Read(_, length) | Self::Stream(_, length)) = self;
        *length
    }

    #[must_use]
    pub fn header(&self) -> HeaderMap {
        let mut header = HeaderMap::new();
        if let Some(length) = self.length() {
            header.typed_insert(length);
        }
        header
    }

    #[must_use]
    pub fn by_read<T: AsyncRead + Send + 'static>(read: T, length: Option<ContentLength>) -> Self {
        Self::Read(Box::pin(read), length)
    }

    #[must_use]
    pub fn by_stream<T: Stream<Item = Result<Bytes, io::Error>> + Send + 'static>(
        length: Option<ContentLength>,
        stream: T,
    ) -> Self {
        Self::Stream(Box::pin(stream), length)
    }

    pub fn into_axum_body(self) -> axum::body::Body {
        match self {
            Self::Axum(stream, _) => axum::body::Body::from_stream(stream),
            Self::Stream(stream, _) => axum::body::Body::from_stream(stream),
            Self::Read(read, _) => axum::body::Body::from_stream(ReaderStream::new(read)),
        }
    }

    #[must_use]
    pub fn into_reqwest_body(self) -> reqwest::Body {
        match self {
            Self::Axum(stream, _) => reqwest::Body::wrap_stream(stream),
            Self::Stream(stream, _) => reqwest::Body::wrap_stream(stream),
            Self::Read(read, _) => reqwest::Body::wrap_stream(ReaderStream::new(read)),
        }
    }

    #[must_use]
    pub fn reader(self) -> StreamReader<Pin<BoxedStream>, Bytes> {
        use futures_util::TryStreamExt as _;
        StreamReader::new(match self {
            Self::Axum(stream, _) => Box::pin(stream.map_err(io::Error::other)),
            Self::Stream(stream, _) => stream,
            Self::Read(read, _) => Box::pin(ReaderStream::new(read)),
        })
    }
}

impl<S: Send + Sync> FromRequest<S> for DataStream {
    type Rejection = ();

    fn from_request(
        req: Request,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> {
        let length = req.headers().typed_get::<ContentLength>();
        std::future::ready(Ok(Self::Axum(req.into_body().into_data_stream(), length)))
    }
}

impl IntoResponse for DataStream {
    fn into_response(self) -> axum::response::Response {
        (self.header(), self.into_axum_body()).into_response()
    }
}

pub trait DatastreamReqwestExt {
    #[must_use]
    fn datastream(self, stream: DataStream) -> Self;
}

impl DatastreamReqwestExt for reqwest::RequestBuilder {
    fn datastream(self, stream: DataStream) -> Self {
        self.headers(stream.header())
            .body(stream.into_reqwest_body())
    }
}
