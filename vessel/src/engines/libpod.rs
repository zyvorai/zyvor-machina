// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Raw Podman libpod HTTP calls over the same unix socket as bollard.

use crate::error::VesselError;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::client::legacy::Client;
use hyperlocal::{UnixClientExt, UnixConnector, Uri};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// Podman compatibility API prefix (Podman 4.x/5.x serve `/v4.0.0/libpod/...`).
const DEFAULT_LIBPOD_PREFIX: &str = "/v4.0.0";

#[derive(Clone)]
pub struct LibpodClient {
    client: Client<UnixConnector, Full<Bytes>>,
    socket: String,
    prefix: String,
}

impl LibpodClient {
    pub fn new(socket: impl Into<String>) -> Self {
        Self {
            client: Client::unix(),
            socket: socket.into(),
            prefix: DEFAULT_LIBPOD_PREFIX.to_string(),
        }
    }

    fn path(&self, suffix: &str) -> String {
        let suf = if suffix.starts_with('/') {
            suffix.to_string()
        } else {
            format!("/{suffix}")
        };
        format!("{}{suf}", self.prefix)
    }

    async fn exchange(
        &self,
        method: Method,
        path: &str,
        body: Option<Bytes>,
    ) -> Result<(StatusCode, Bytes), VesselError> {
        let full = self.path(path);
        let uri = Uri::new(&self.socket, &full);
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Full::new(body.unwrap_or_default()))
            .map_err(|e| VesselError::Internal(e.to_string()))?;

        let resp: Response<Incoming> = self
            .client
            .request(req)
            .await
            .map_err(|e| VesselError::Api(format!("libpod request failed: {e}")))?;

        let status = resp.status();
        let collected = resp
            .into_body()
            .collect()
            .await
            .map_err(|e| VesselError::Api(format!("libpod body: {e}")))?;
        Ok((status, collected.to_bytes()))
    }

    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, VesselError> {
        let (status, bytes) = self.exchange(Method::GET, path, None).await?;
        if status == StatusCode::NOT_FOUND {
            return Err(VesselError::NotFound(self.path(path)));
        }
        if !status.is_success() {
            return Err(VesselError::Api(format!(
                "libpod GET {}: HTTP {status}: {}",
                self.path(path),
                String::from_utf8_lossy(&bytes)
            )));
        }
        serde_json::from_slice(&bytes)
            .map_err(|e| VesselError::Api(format!("libpod decode {}: {e}", self.path(path))))
    }

    pub async fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, VesselError> {
        let payload = serde_json::to_vec(body)
            .map_err(|e| VesselError::Internal(format!("serialize: {e}")))?;
        let (status, bytes) = self
            .exchange(Method::POST, path, Some(Bytes::from(payload)))
            .await?;
        if status == StatusCode::NOT_FOUND {
            return Err(VesselError::NotFound(self.path(path)));
        }
        if !status.is_success() {
            return Err(VesselError::Api(format!(
                "libpod POST {}: HTTP {status}: {}",
                self.path(path),
                String::from_utf8_lossy(&bytes)
            )));
        }
        if bytes.is_empty() {
            return Err(VesselError::Api(format!(
                "libpod POST {}: empty JSON body",
                self.path(path)
            )));
        }
        serde_json::from_slice(&bytes)
            .map_err(|e| VesselError::Api(format!("libpod decode {}: {e}", self.path(path))))
    }

    pub async fn post_empty(&self, path: &str) -> Result<(), VesselError> {
        let (status, bytes) = self.exchange(Method::POST, path, None).await?;
        if status == StatusCode::NOT_FOUND {
            return Err(VesselError::NotFound(self.path(path)));
        }
        if !status.is_success() {
            return Err(VesselError::Api(format!(
                "libpod POST {}: HTTP {status}: {}",
                self.path(path),
                String::from_utf8_lossy(&bytes)
            )));
        }
        Ok(())
    }

    pub async fn delete(&self, path: &str) -> Result<(), VesselError> {
        let (status, bytes) = self.exchange(Method::DELETE, path, None).await?;
        if status == StatusCode::NOT_FOUND {
            return Err(VesselError::NotFound(self.path(path)));
        }
        if !status.is_success() {
            return Err(VesselError::Api(format!(
                "libpod DELETE {}: HTTP {status}: {}",
                self.path(path),
                String::from_utf8_lossy(&bytes)
            )));
        }
        Ok(())
    }
}
