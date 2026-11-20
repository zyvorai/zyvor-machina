// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! FluxVM backend glue: client from `[fluxvm]`, fail-soft listing for the merged
//! VM list, and `GET /api/v1/fluxvm/status` for the UI.

use axum::routing::get;
use axum::{Json, Router};
use machina_core::fluxvm::FluxvmClient;
use machina_core::{LibvirtError, LibvirtManager, MachinaConfig, VmInfo};
use serde_json::{json, Value};

use crate::error::AppError;

pub(crate) type Upstream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Open a `fluxvm-api` WebSocket (serial / agent console). `wss://` verifies
/// against the native roots unless `[fluxvm] insecure_tls` is set.
pub(crate) async fn connect_ws(url: &str, token: Option<&str>) -> Result<Upstream, String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut req = url
        .into_client_request()
        .map_err(|e| format!("bad FluxVM URL: {e}"))?;
    if let Some(t) = token {
        if let Ok(v) = format!("Bearer {t}").parse() {
            req.headers_mut().insert("authorization", v);
        }
    }
    let connector = if url.starts_with("wss://") && MachinaConfig::load().fluxvm.insecure_tls {
        Some(tokio_tungstenite::Connector::Rustls(insecure_rustls()?))
    } else {
        None
    };
    tokio_tungstenite::connect_async_tls_with_config(req, None, false, connector)
        .await
        .map(|(s, _)| s)
        .map_err(|e| e.to_string())
}

fn insecure_rustls() -> Result<std::sync::Arc<rustls::ClientConfig>, String> {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use std::sync::Arc;

    #[derive(Debug)]
    struct NoVerify(Arc<rustls::crypto::CryptoProvider>);
    impl ServerCertVerifier for NoVerify {
        fn verify_server_cert(
            &self,
            _: &CertificateDer<'_>,
            _: &[CertificateDer<'_>],
            _: &ServerName<'_>,
            _: &[u8],
            _: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &rustls::DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &rustls::DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerify(provider)))
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

pub(crate) fn client() -> Result<FluxvmClient, AppError> {
    FluxvmClient::from_config(&MachinaConfig::load().fluxvm).map_err(AppError::from)
}

/// FluxVM VMs for the merged list; empty (with a warning) when disabled or unreachable.
pub(crate) async fn list_vm_infos() -> Vec<VmInfo> {
    let cfg = MachinaConfig::load().fluxvm;
    if !cfg.enabled {
        return Vec::new();
    }
    let c = match FluxvmClient::from_config(&cfg) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("fluxvm: {e}");
            return Vec::new();
        }
    };
    match c.list().await {
        Ok(items) => items.iter().map(|r| r.to_vm_info()).collect(),
        Err(e) => {
            tracing::warn!("fluxvm: list failed, showing libvirt VMs only: {e}");
            Vec::new()
        }
    }
}

/// Merge FluxVM VMs into a libvirt list; libvirt wins on a name clash.
pub(crate) async fn merge_into(mut vms: Vec<VmInfo>) -> Vec<VmInfo> {
    let flux = list_vm_infos().await;
    for f in flux {
        if !vms.iter().any(|v| v.name == f.name) {
            vms.push(f);
        }
    }
    vms
}

/// Resize a FluxVM VM to an absolute size. FluxVM only hot-adds (QEMU and
/// Cloud Hypervisor), so a smaller target is refused.
pub(crate) async fn resize(
    name: &str,
    vcpus: Option<u32>,
    memory_mb: Option<u64>,
) -> Result<Value, AppError> {
    let c = client()?;
    let rec = c.get(name).await?;
    let mut out = json!({ "status": "ok", "name": name, "live": true, "backend": "fluxvm" });
    if let Some(want) = vcpus {
        let have = rec.live_vcpus();
        if want < have {
            return Err(LibvirtError::Invalid(format!(
                "FluxVM can only add vCPUs ({have} → {want}); stop the VM and re-create it to shrink"
            ))
            .into());
        }
        let now = if want > have {
            c.hotplug_cpu(name, want - have).await?
        } else {
            have
        };
        out["vcpus"] = now.into();
    }
    if let Some(want) = memory_mb {
        let have = rec.live_memory_mib();
        if want < have {
            return Err(LibvirtError::Invalid(format!(
                "FluxVM can only add memory ({have} → {want} MiB); stop the VM and re-create it to shrink"
            ))
            .into());
        }
        let now = if want > have {
            c.hotplug_memory(name, want - have).await?
        } else {
            have
        };
        out["memory_mb"] = now.into();
        out["live_applied"] = true.into();
    }
    Ok(out)
}

async fn fluxvm_status() -> Json<Value> {
    let cfg = MachinaConfig::load().fluxvm;
    let mut out = json!({
        "enabled": cfg.enabled,
        "base_url": cfg.base_url,
        "default_backend": cfg.default_backend,
        "reachable": false,
    });
    if !cfg.enabled {
        return Json(out);
    }
    let c = match FluxvmClient::from_config(&cfg) {
        Ok(c) => c,
        Err(e) => {
            out["last_error"] = e.to_string().into();
            return Json(out);
        }
    };
    match c.health().await {
        Ok(_) => {
            out["reachable"] = true.into();
            if let Ok(caps) = c.capabilities().await {
                out["capabilities"] = caps;
            }
        }
        Err(e) => out["last_error"] = e.to_string().into(),
    }
    Json(out)
}

pub fn fluxvm_routes() -> Router<LibvirtManager> {
    Router::new().route("/fluxvm/status", get(fluxvm_status))
}
