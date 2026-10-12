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
    name.starts_with("metal-") || name.starts_with("install-")
}

pub fn dir() -> std::path::PathBuf {
    std::env::var_os("MACHINA_DIST_DIR")
        .map(Into::into)
        .unwrap_or_else(|| "/var/lib/machina/dist".into())
}

pub fn allowed(name: &str) -> bool {
    FILES.contains(&name) || is_installer_asset(name)
}

/// `install-<os>-vmlinuz`, `-initrd`, `.iso`: installer media the operator copies in.
fn is_installer_asset(name: &str) -> bool {
    name.starts_with("install-")
        && name.len() <= 96
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && (name.ends_with("-vmlinuz") || name.ends_with("-initrd") || name.ends_with(".iso"))
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

/// `GET /dist/images/{name}`: an imported OS image, streamed, with `Range: bytes=N-` so a
/// machine's deploy script can resume a half-finished download.
pub async fn serve_image(Path(name): Path<String>, headers: axum::http::HeaderMap) -> Response {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    if !image_name_ok(&name) {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let path = crate::engine::baremetal_images::images_dir().join(&name);
    let mut f = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return (StatusCode::NOT_FOUND, "image not found").into_response(),
    };
    let total = match f.metadata().await {
        Ok(m) => m.len(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "stat failed").into_response(),
    };
    let start = parse_range_start(
        headers.get(header::RANGE).and_then(|v| v.to_str().ok()),
        total,
    );
    let partial = start.is_some();
    let start = start.unwrap_or(0);
    if start >= total && total > 0 {
        return (
            StatusCode::RANGE_NOT_SATISFIABLE,
            [(header::CONTENT_RANGE, format!("bytes */{total}"))],
        )
            .into_response();
    }
    if start > 0 && f.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "seek failed").into_response();
    }
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
    let mut resp = Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, total - start);
    if partial {
        resp = resp.status(StatusCode::PARTIAL_CONTENT).header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{total}", total.saturating_sub(1)),
        );
    }
    resp.body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn image_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.contains("..")
        && !name.ends_with(".part")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Start offset of a single `bytes=N-` (or `bytes=N-M`) range; `None` when absent or unusable.
fn parse_range_start(h: Option<&str>, total: u64) -> Option<u64> {
    let v = h?.trim().strip_prefix("bytes=")?;
    if v.contains(',') {
        return None;
    }
    let (a, _) = v.split_once('-')?;
    let n: u64 = a.trim().parse().ok()?;
    let _ = total;
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::{allowed, image_name_ok, parse_range_start};

    #[test]
    fn image_names_and_ranges() {
        assert!(image_name_ok(
            "ubuntu-26.04-ubuntu-26.04-server-cloudimg-amd64.img"
        ));
        for bad in ["", "../x", "a/b", "x.part", ".."] {
            assert!(!image_name_ok(bad), "{bad}");
        }
        assert_eq!(parse_range_start(Some("bytes=100-"), 1000), Some(100));
        assert_eq!(parse_range_start(Some("bytes=5-9"), 1000), Some(5));
        assert_eq!(parse_range_start(Some("bytes=1-2,5-6"), 1000), None);
        assert_eq!(parse_range_start(None, 1000), None);
        assert_eq!(parse_range_start(Some("items=1-"), 1000), None);
    }

    #[test]
    fn only_the_published_names_are_served() {
        for ok in [
            "machina-agent",
            "SHA256SUMS",
            "machina-bpfd.service",
            "install-ubuntu-24.04-vmlinuz",
            "install-rocky-9-initrd",
            "install-ubuntu-24.04.iso",
        ] {
            assert!(allowed(ok), "{ok}");
        }
        for bad in [
            "../etc/passwd",
            "machina-controller",
            "",
            "machina-agent/",
            "/etc/passwd",
            "machina-agent.bak",
            "install-../x-vmlinuz",
            "install-ubuntu/x-vmlinuz",
            "install-ubuntu-24.04.tar",
        ] {
            assert!(!allowed(bad), "{bad}");
        }
    }
}
