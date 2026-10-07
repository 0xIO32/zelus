// SPDX-License-Identifier: AGPL-3.0-only
use crate::server::{CertResolver, TlsAcceptor, server_config};
use crate::{DaemonKey, DaemonKeyError, DaemonRootKey, DaemonRootKeyError};
use axum::Router;
use axum_server::Server;
use core::net::SocketAddr;
use core::time::Duration;
use rcgen::{KeyPair, PKCS_ECDSA_P256_SHA256, PKCS_RSA_SHA256};
use reqwest::{Client, StatusCode};
use rustls::pki_types::{CertificateDer, CertificateSigningRequestDer};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use url::Url;

const PREFER_RSA_HEADER: &str = "X-Prefer-RSA-Key";
const CONNECTION_TEST_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("invalid setup url: {0}")]
    Url(#[from] url::ParseError),
    #[error("request to the backend failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid root key: {0}")]
    RootKey(#[from] DaemonRootKeyError),
    #[error("invalid key: {0}")]
    Key(#[from] DaemonKeyError),
    #[error("unable to generate a key: {0}")]
    Rcgen(#[from] rcgen::Error),
    #[error("unable to build the tls config: {0}")]
    Tls(#[from] rustls::Error),
}

pub struct SetupClient {
    client: Client,
    endpoint: Url,
    code: String,
}

pub struct RootKeyOffer {
    pub root: DaemonRootKey,
    pub prefers_rsa: bool,
}

impl SetupClient {
    pub fn new(
        client: Client,
        backend: &Url,
        endpoint: &str,
        code: &str,
    ) -> Result<Self, SetupError> {
        Ok(Self {
            client,
            endpoint: backend.join(&format!("{}/", endpoint.trim_end_matches('/')))?,
            code: code.to_owned(),
        })
    }

    pub async fn root_key(&self) -> Result<Option<RootKeyOffer>, SetupError> {
        let response = self
            .client
            .get(self.endpoint.join("root_key")?)
            .bearer_auth(&self.code)
            .send()
            .await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Ok(None);
        }

        let response = response.error_for_status()?;
        let prefers_rsa = response.headers().contains_key(PREFER_RSA_HEADER);
        let pem: String = response.json().await?;

        Ok(Some(RootKeyOffer {
            root: DaemonRootKey::parse(pem)?.0,
            prefers_rsa,
        }))
    }

    pub async fn config<T: DeserializeOwned>(&self) -> Result<T, SetupError> {
        Ok(self
            .client
            .get(self.endpoint.join("config")?)
            .bearer_auth(&self.code)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn register<R: Serialize + Sync>(
        &self,
        request: &R,
    ) -> Result<CertificateDer<'static>, SetupError> {
        let signed = self
            .client
            .post(self.endpoint.join("register")?)
            .bearer_auth(&self.code)
            .json(request)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;

        Ok(CertificateDer::from(signed.to_vec()))
    }
}

pub struct PendingKey {
    pub temp: Arc<DaemonKey>,
    pub csr: CertificateSigningRequestDer<'static>,
    prefix: String,
}

impl PendingKey {
    pub fn generate(prefers_rsa: bool, prefix: &str) -> Result<Self, SetupError> {
        let keypair = KeyPair::generate_for(if prefers_rsa {
            &PKCS_RSA_SHA256
        } else {
            &PKCS_ECDSA_P256_SHA256
        })?;

        Ok(Self {
            temp: Arc::new(DaemonKey::new_temp(&keypair, prefix)?),
            csr: DaemonKey::new_request(&keypair, prefix)?,
            prefix: prefix.to_owned(),
        })
    }

    #[must_use]
    pub fn certificate(&self) -> &[u8] {
        self.temp.cert.first().map_or(&[], |cert| cert.as_ref())
    }

    pub fn finish(
        &self,
        root: &DaemonRootKey,
        signed: CertificateDer<'static>,
    ) -> Result<DaemonKey, SetupError> {
        Ok(DaemonKey::new(
            root,
            signed,
            self.temp.privkey.clone_key(),
            &self.prefix,
        )?)
    }
}

pub async fn serve_connection_test(
    binds: impl IntoIterator<Item = SocketAddr>,
    root: &DaemonRootKey,
    key: &DaemonKey,
    router: Router,
) -> Result<(), SetupError> {
    let config = server_config(root, CertResolver::new(key))?;

    for bind in binds {
        let router = router.clone();
        let acceptor = TlsAcceptor::new(config.clone());
        tokio::spawn(async move {
            let _ = Server::bind(bind)
                .acceptor(acceptor)
                .serve(router.into_make_service())
                .await;
        });
    }

    tokio::time::sleep(CONNECTION_TEST_GRACE).await;

    Ok(())
}
