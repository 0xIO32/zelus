// SPDX-License-Identifier: AGPL-3.0-only
use crate::{DaemonKey, DaemonRootKey};
use axum::Extension;
use axum::middleware::AddExtension;
use axum_server::accept::Accept;
use axum_server::tls_rustls::{RustlsAcceptor, RustlsConfig};
use core::fmt::{Debug, Formatter};
use futures_util::future::BoxFuture;
use rustls::ServerConfig;
use rustls::pki_types::CertificateDer;
use rustls::server::{ClientHello, ResolvesServerCert, WebPkiClientVerifier};
use rustls::sign::CertifiedKey;
use std::io;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::server::TlsStream;
use tower_layer::Layer as _;

pub struct CertResolver {
    key: Arc<CertifiedKey>,
    server_name: String,
    fallback: Option<Arc<CertifiedKey>>,
}

impl CertResolver {
    #[must_use]
    pub fn new(key: &DaemonKey) -> Self {
        Self {
            key: Arc::clone(&key.tls_cert),
            server_name: key.server_name.clone(),
            fallback: None,
        }
    }

    #[must_use]
    pub fn with_fallback(mut self, fallback: Option<Arc<CertifiedKey>>) -> Self {
        self.fallback = fallback;
        self
    }
}

impl ResolvesServerCert for CertResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        match &self.fallback {
            Some(fallback) if client_hello.server_name() != Some(self.server_name.as_str()) => {
                Some(Arc::clone(fallback))
            }
            _ => Some(Arc::clone(&self.key)),
        }
    }
}

impl Debug for CertResolver {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("CertResolver")
    }
}

pub fn server_config(
    root: &DaemonRootKey,
    resolver: CertResolver,
) -> Result<RustlsConfig, rustls::Error> {
    let verifier = WebPkiClientVerifier::builder(Arc::clone(&root.cert_store))
        .allow_unauthenticated()
        .allow_unknown_revocation_status()
        .build()
        .map_err(|err| rustls::Error::General(err.to_string()))?;

    let mut config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_cert_resolver(Arc::new(resolver));
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(RustlsConfig::from_config(Arc::new(config)))
}

#[derive(Debug, Clone)]
pub struct TlsData {
    pub peer_certificates: Option<Vec<CertificateDer<'static>>>,
}

#[derive(Debug, Clone)]
pub struct TlsAcceptor<A>(RustlsAcceptor<A>);

impl TlsAcceptor<axum_server::accept::DefaultAcceptor> {
    #[must_use]
    pub fn new(config: RustlsConfig) -> Self {
        Self(RustlsAcceptor::new(config))
    }
}

impl<A, I, S> Accept<I, S> for TlsAcceptor<A>
where
    A: Accept<I, S> + Clone + Send + 'static,
    A::Stream: Send + AsyncRead + AsyncWrite + Unpin,
    A::Service: Send,
    A::Future: Send,
    I: Send + 'static,
    S: Send + 'static,
{
    type Stream = TlsStream<A::Stream>;
    type Service = AddExtension<A::Service, TlsData>;
    type Future = BoxFuture<'static, io::Result<(Self::Stream, Self::Service)>>;

    fn accept(&self, stream: I, service: S) -> Self::Future {
        let acceptor = self.0.clone();

        Box::pin(async move {
            let (stream, service) = acceptor.accept(stream, service).await?;
            let tls_data = TlsData {
                peer_certificates: stream.get_ref().1.peer_certificates().map(From::from),
            };

            Ok((stream, Extension(tls_data).layer(service)))
        })
    }
}
