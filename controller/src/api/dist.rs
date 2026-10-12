// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! `/dist/*`: the agent binaries and unit files a bare node downloads in `install.sh`, published
//! by `machinactl dist publish` into `MACHINA_DIST_DIR` (default /var/lib/machina/dist) together
//! with a SHA256SUMS file. Only these names are served; the node verifies every file against
//! SHA256SUMS over a channel pinned to the controller's CA.

use axum::body::Body;
use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

pub const FILES: &[&str] = &[
    "machina-agent",
    "machina-bpfd",
    "machina-agent.service",
    "machina-bpfd.service",
    "SHA256SUMS",
    // Commissioning image for bare-metal PXE boot (large: streamed, not read into memory).
    "metal-vmlinuz",
    "metal-initrd",
    "metal-rootfs.squashfs",
];

fn is_large(name: &str) -> bool {
    name.starts_with("metal-")
}

pub fn dir() -> std::path::PathBuf {
    std::env::var_os("MACHINA_DIST_DIR")
        .map(Into::into)
        .unwrap_or_else(|| "/var/lib/machina/dist".into())
}

pub fn allowed(name: &str) -> bool {
    FILES.contains(&name)
}

pub async fn serve(Path(name): Path<String>) -> Response {
    if !allowed(&name) {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    if is_large(&name) {
        return match tokio::fs::File::open(dir().join(&name)).await {
            Ok(f) => {
                use tokio::io::AsyncReadExt;
                let stream = futures_util::stream::unfold(f, |mut f| async move {
                    let mut buf = vec![0u8; 256 * 1024];
                    match f.read(&mut buf).await {
                        Ok(0) | Err(_) => None,
                        Ok(n) => {
                            buf.truncate(n);
                            Some((Ok::<_, std::io::Error>(bytes::Bytes::from(buf)), f))
                        }
                    }
                });
                (
                    [(header::CONTENT_TYPE, "application/octet-stream")],
                    Body::from_stream(stream),
                )
                    .into_response()
            }
            Err(_) => (
                StatusCode::NOT_FOUND,
                "commissioning image not published: copy it to MACHINA_DIST_DIR",
            )
                .into_response(),
        };
    }
    match tokio::fs::read(dir().join(&name)).await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "application/octet-stream")],
            Body::from(bytes),
        )
            .into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            "nothing published: run `machinactl dist publish` on the controller",
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::allowed;

    #[test]
    fn only_the_published_names_are_served() {
        for ok in ["machina-agent", "SHA256SUMS", "machina-bpfd.service"] {
            assert!(allowed(ok), "{ok}");
        }
        for bad in [
            "../etc/passwd",
            "machina-controller",
            "",
            "machina-agent/",
            "/etc/passwd",
            "machina-agent.bak",
        ] {
            assert!(!allowed(bad), "{bad}");
        }
    }
}
