// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! The controller's network listener for joining nodes, off unless `MACHINA_CONTROLLER_TLS_ADDR`
//! is set (for example `0.0.0.0:5094`). It serves HTTPS with a certificate from the fleet CA and
//! exposes only what a joining node needs (join, the CA, health, the install script): the rest of
//! the API stays on the controller's own listener. A node pins the CA by its fingerprint.

use std::net::SocketAddr;

use axum::http::{header, HeaderValue};
use axum_server::tls_rustls::RustlsConfig;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::api;
use crate::state::AppState;

pub fn configured_addr() -> Option<String> {
    std::env::var("MACHINA_CONTROLLER_TLS_ADDR")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Names the listener's certificate must carry: the host of the public URL, any extras in
/// `MACHINA_CONTROLLER_TLS_SANS` (comma separated), and loopback.
pub fn sans_for(public_url: &str, extra: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(h) = url::Url::parse(public_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
    {
        out.push(h.trim_start_matches('[').trim_end_matches(']').to_string());
    }
    out.extend(
        extra
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    );
    out.extend(["localhost".to_string(), "127.0.0.1".to_string()]);
    let mut seen = std::collections::HashSet::new();
    out.retain(|s| seen.insert(s.clone()));
    out
}

/// `https://<public host>:<tls port>` for the join command.
pub fn public_https_url(public_url: &str, tls_addr: &str) -> Option<String> {
    let host = url::Url::parse(public_url).ok()?.host_str()?.to_string();
    let port = tls_addr.rsplit_once(':')?.1;
    // `host_str()` already brackets an IPv6 literal.
    Some(format!("https://{host}:{port}"))
}

pub async fn serve(state: AppState, addr: SocketAddr, public_url: String) -> anyhow::Result<()> {
    let sans = sans_for(
        &public_url,
        &std::env::var("MACHINA_CONTROLLER_TLS_SANS").unwrap_or_default(),
    );
    let id = crate::pki::server_identity(&sans)?;
    let (_, fp) = crate::pki::ca_info()?;
    let cfg = RustlsConfig::from_pem(id.cert_pem.into_bytes(), id.key_pem.into_bytes()).await?;
    let app = api::enrollment_router(state)
        .layer(TraceLayer::new_for_http())
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ));
    tracing::info!(
        "join listener on https://{addr} (names: {}); nodes pin the CA with --ca-sha256 {fp}",
        sans.join(", ")
    );
    axum_server::bind_rustls(addr, cfg)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_come_from_the_public_url_extras_and_loopback_without_duplicates() {
        let s = sans_for(
            "https://ctl.example.com:5093",
            "10.0.0.5, ctl.example.com ,",
        );
        assert_eq!(s, ["ctl.example.com", "10.0.0.5", "localhost", "127.0.0.1"]);
        assert_eq!(
            sans_for("http://10.1.1.1:5093", ""),
            ["10.1.1.1", "localhost", "127.0.0.1"]
        );
        assert_eq!(sans_for("nonsense", ""), ["localhost", "127.0.0.1"]);
    }

    #[test]
    fn the_join_url_uses_the_public_host_and_the_tls_port() {
        assert_eq!(
            public_https_url("http://ctl.example.com:5093", "0.0.0.0:5094").as_deref(),
            Some("https://ctl.example.com:5094")
        );
        assert_eq!(
            public_https_url("http://[fd00::1]:5093", "[::]:5094").as_deref(),
            Some("https://[fd00::1]:5094")
        );
        assert_eq!(public_https_url("nonsense", "0.0.0.0:5094"), None);
    }
}
