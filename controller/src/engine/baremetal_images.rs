// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! OS image catalog for bare-metal deploys: well-known cloud images (Ubuntu, Debian,
//! Rocky, AlmaLinux) by name, or any URL, always checked against a checksum before the
//! image is marked `ready`. The checksum comes from the upstream checksum file (found by
//! name) or from the operator. For an air-gapped site, import from an internal mirror by
//! URL with an explicit checksum.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use uuid::Uuid;

use crate::db::DbPool;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Algo {
    Sha256,
    Sha512,
}

impl Algo {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
            Self::Sha512 => "sha512",
        }
    }

    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "sha256" => Ok(Self::Sha256),
            "sha512" => Ok(Self::Sha512),
            other => anyhow::bail!("unsupported checksum algorithm '{other}' (sha256, sha512)"),
        }
    }

    fn hex_len(self) -> usize {
        match self {
            Self::Sha256 => 64,
            Self::Sha512 => 128,
        }
    }
}

enum Hasher {
    S256(Sha256),
    S512(Sha512),
}

impl Hasher {
    fn new(a: Algo) -> Self {
        match a {
            Algo::Sha256 => Self::S256(Sha256::new()),
            Algo::Sha512 => Self::S512(Sha512::new()),
        }
    }
    fn update(&mut self, b: &[u8]) {
        match self {
            Self::S256(h) => h.update(b),
            Self::S512(h) => h.update(b),
        }
    }
    fn finish(self) -> String {
        match self {
            Self::S256(h) => hex::encode(h.finalize()),
            Self::S512(h) => hex::encode(h.finalize()),
        }
    }
}

pub struct CatalogImage {
    pub name: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub url: &'static str,
    pub sums_url: &'static str,
    pub algo: Algo,
    pub format: &'static str,
}

/// Upstream cloud images. `name` is what operators type (`metal image add ubuntu-26.04`).
pub const CATALOG: &[CatalogImage] = &[
    CatalogImage {
        name: "ubuntu-26.04",
        os: "ubuntu",
        arch: "amd64",
        url: "https://cloud-images.ubuntu.com/releases/26.04/release/ubuntu-26.04-server-cloudimg-amd64.img",
        sums_url: "https://cloud-images.ubuntu.com/releases/26.04/release/SHA256SUMS",
        algo: Algo::Sha256,
        format: "qcow2",
    },
    CatalogImage {
        name: "ubuntu-26.04-arm64",
        os: "ubuntu",
        arch: "arm64",
        url: "https://cloud-images.ubuntu.com/releases/26.04/release/ubuntu-26.04-server-cloudimg-arm64.img",
        sums_url: "https://cloud-images.ubuntu.com/releases/26.04/release/SHA256SUMS",
        algo: Algo::Sha256,
        format: "qcow2",
    },
    CatalogImage {
        name: "ubuntu-24.04",
        os: "ubuntu",
        arch: "amd64",
        url: "https://cloud-images.ubuntu.com/releases/24.04/release/ubuntu-24.04-server-cloudimg-amd64.img",
        sums_url: "https://cloud-images.ubuntu.com/releases/24.04/release/SHA256SUMS",
        algo: Algo::Sha256,
        format: "qcow2",
    },
    CatalogImage {
        name: "ubuntu-22.04",
        os: "ubuntu",
        arch: "amd64",
        url: "https://cloud-images.ubuntu.com/releases/22.04/release/ubuntu-22.04-server-cloudimg-amd64.img",
        sums_url: "https://cloud-images.ubuntu.com/releases/22.04/release/SHA256SUMS",
        algo: Algo::Sha256,
        format: "qcow2",
    },
    CatalogImage {
        name: "debian-12",
        os: "debian",
        arch: "amd64",
        url: "https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-genericcloud-amd64.qcow2",
        sums_url: "https://cloud.debian.org/images/cloud/bookworm/latest/SHA512SUMS",
        algo: Algo::Sha512,
        format: "qcow2",
    },
    CatalogImage {
        name: "debian-13",
        os: "debian",
        arch: "amd64",
        url: "https://cloud.debian.org/images/cloud/trixie/latest/debian-13-genericcloud-amd64.qcow2",
        sums_url: "https://cloud.debian.org/images/cloud/trixie/latest/SHA512SUMS",
        algo: Algo::Sha512,
        format: "qcow2",
    },
    CatalogImage {
        name: "rocky-9",
        os: "rocky",
        arch: "amd64",
        url: "https://dl.rockylinux.org/pub/rocky/9/images/x86_64/Rocky-9-GenericCloud-Base.latest.x86_64.qcow2",
        sums_url: "https://dl.rockylinux.org/pub/rocky/9/images/x86_64/CHECKSUM",
        algo: Algo::Sha256,
        format: "qcow2",
    },
    CatalogImage {
        name: "almalinux-9",
        os: "almalinux",
        arch: "amd64",
        url: "https://repo.almalinux.org/almalinux/9/cloud/x86_64/images/AlmaLinux-9-GenericCloud-latest.x86_64.qcow2",
        sums_url: "https://repo.almalinux.org/almalinux/9/cloud/x86_64/images/CHECKSUM",
        algo: Algo::Sha256,
        format: "qcow2",
    },
];

pub fn catalog_entry(name: &str) -> Option<&'static CatalogImage> {
    CATALOG.iter().find(|c| c.name == name)
}

/// The hash for `file` in a checksum document. Understands GNU (`<hex> *name`,
/// `<hex>  name`) and BSD (`SHA256 (name) = <hex>`) layouts.
pub fn parse_sums(text: &str, file: &str) -> Option<String> {
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        // BSD
        if let Some(rest) = l.split_once(" (").map(|(_, r)| r) {
            if let Some((name, tail)) = rest.split_once(") = ") {
                if name == file {
                    let h = tail.trim().to_ascii_lowercase();
                    if !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Some(h);
                    }
                }
                continue;
            }
        }
        // GNU
        let mut it = l.splitn(2, char::is_whitespace);
        let (Some(h), Some(name)) = (it.next(), it.next()) else {
            continue;
        };
        let name = name.trim().trim_start_matches('*');
        if name == file && h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(h.to_ascii_lowercase());
        }
    }
    None
}

pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
        && !n.starts_with('.')
}

fn url_file_name(url: &str) -> String {
    url.split(['?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    /// A catalog name (`ubuntu-26.04`) or the name to give a URL import.
    pub name: String,
    #[serde(default)]
    pub url: String,
    /// Hex digest of the file. Required for a URL import unless `sums_url` is given.
    #[serde(default)]
    pub checksum: String,
    #[serde(default)]
    pub checksum_algo: String,
    /// A checksum file (GNU or BSD layout) to look the image's file name up in.
    #[serde(default)]
    pub sums_url: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub format: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct BaremetalImage {
    pub id: Uuid,
    pub name: String,
    pub os: String,
    pub arch: String,
    pub format: String,
    pub size_bytes: i64,
    pub checksum: String,
    pub checksum_algo: String,
    pub source_url: String,
    pub status: String,
    pub error: String,
    pub created_at: String,
}

const COLS: &str = "id, name, os, arch, format, size_bytes, checksum, checksum_algo, source_url, status, error, created_at";

pub fn images_dir() -> PathBuf {
    crate::api::dist::dir().join("images")
}

pub async fn list(pool: &DbPool) -> anyhow::Result<Vec<BaremetalImage>> {
    Ok(crate::db::query_as::<_, BaremetalImage>(&format!(
        "SELECT {COLS} FROM baremetal_images ORDER BY name"
    ))
    .fetch_all(pool)
    .await?)
}

pub async fn get(pool: &DbPool, name: &str) -> anyhow::Result<Option<BaremetalImage>> {
    Ok(crate::db::query_as::<_, BaremetalImage>(&format!(
        "SELECT {COLS} FROM baremetal_images WHERE name = ?"
    ))
    .bind(name)
    .fetch_optional(pool)
    .await?)
}

/// What the background task needs once the request is validated.
struct Plan {
    url: String,
    expected: Expected,
    algo: Algo,
    file_name: String,
    format: String,
    os: String,
    arch: String,
}

enum Expected {
    Hex(String),
    FromSums { url: String, file: String },
}

fn plan(req: &ImportRequest) -> anyhow::Result<Plan> {
    if !valid_name(&req.name) {
        anyhow::bail!("name must be 1-64 characters of a-z, 0-9, '.', '-', '_'");
    }
    if req.url.trim().is_empty() {
        let c = catalog_entry(&req.name).ok_or_else(|| {
            anyhow::anyhow!(
                "'{}' is not in the catalog; give a url and a checksum (or sums_url)",
                req.name
            )
        })?;
        return Ok(Plan {
            url: c.url.into(),
            expected: if req.checksum.trim().is_empty() {
                Expected::FromSums {
                    url: c.sums_url.into(),
                    file: url_file_name(c.url),
                }
            } else {
                Expected::Hex(req.checksum.trim().to_ascii_lowercase())
            },
            algo: c.algo,
            file_name: url_file_name(c.url),
            format: c.format.into(),
            os: c.os.into(),
            arch: c.arch.into(),
        });
    }
    let url = req.url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        anyhow::bail!("url must be http(s)");
    }
    let algo = Algo::parse(&req.checksum_algo)?;
    let file_name = url_file_name(url);
    let expected = if !req.checksum.trim().is_empty() {
        Expected::Hex(req.checksum.trim().to_ascii_lowercase())
    } else if !req.sums_url.trim().is_empty() {
        Expected::FromSums {
            url: req.sums_url.trim().to_string(),
            file: file_name.clone(),
        }
    } else {
        anyhow::bail!(
            "a url import needs a checksum or a sums_url: images are never written unverified"
        );
    };
    Ok(Plan {
        url: url.into(),
        expected,
        algo,
        file_name,
        format: if req.format.is_empty() {
            "qcow2".into()
        } else {
            req.format.clone()
        },
        os: req.os.clone(),
        arch: if req.arch.is_empty() {
            "amd64".into()
        } else {
            req.arch.clone()
        },
    })
}

fn safe_file_component(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && !s.contains("..")
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Validate, record the image as `downloading`, and fetch it in the background.
pub async fn start_import(pool: &DbPool, req: &ImportRequest) -> anyhow::Result<BaremetalImage> {
    let p = plan(req)?;
    if !safe_file_component(&p.file_name) {
        anyhow::bail!("cannot derive a safe file name from the url");
    }
    if let Some(existing) = get(pool, &req.name).await? {
        if existing.status != "failed" {
            anyhow::bail!("image '{}' already exists ({})", req.name, existing.status);
        }
        crate::db::query("DELETE FROM baremetal_images WHERE name = ?")
            .bind(&req.name)
            .execute(pool)
            .await?;
    }
    let id = Uuid::new_v4();
    let dir = images_dir();
    let rel = format!("{}-{}", req.name, p.file_name);
    crate::db::query(
        "INSERT INTO baremetal_images
         (id, name, os, arch, format, checksum_algo, source_url, path, status)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'downloading')",
    )
    .bind(id)
    .bind(&req.name)
    .bind(&p.os)
    .bind(&p.arch)
    .bind(&p.format)
    .bind(p.algo.as_str())
    .bind(&p.url)
    .bind(&rel)
    .execute(pool)
    .await?;

    let pool2 = pool.clone();
    tokio::spawn(async move {
        if let Err(e) = run_import(&pool2, id, &dir, &rel, &p).await {
            let _ = crate::db::query(
                "UPDATE baremetal_images SET status = 'failed', error = ? WHERE id = ?",
            )
            .bind(format!("{e:#}"))
            .bind(id)
            .execute(&pool2)
            .await;
            let _ = tokio::fs::remove_file(dir.join(format!("{rel}.part"))).await;
        }
    });
    get(pool, &req.name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("image row vanished"))
}

fn max_bytes() -> u64 {
    std::env::var("MACHINA_METAL_IMAGE_MAX_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20 * 1024 * 1024 * 1024)
}

fn retry_pause(attempt: u32) -> Duration {
    if cfg!(test) {
        Duration::from_millis(20)
    } else {
        Duration::from_secs(5 * u64::from(attempt))
    }
}

/// One transfer attempt: stream `url` to `part`, hashing as it goes. Returns (hex digest, bytes).
async fn download(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    algo: Algo,
    limit: u64,
) -> anyhow::Result<(String, u64)> {
    use tokio::io::AsyncWriteExt;
    let mut resp = client.get(url).send().await?.error_for_status()?;
    if resp.content_length().is_some_and(|n| n > limit) {
        anyhow::bail!("image is larger than MACHINA_METAL_IMAGE_MAX_BYTES ({limit})");
    }
    let mut f = tokio::fs::File::create(part).await?;
    let mut h = Hasher::new(algo);
    let mut size: u64 = 0;
    while let Some(chunk) = resp.chunk().await? {
        size += chunk.len() as u64;
        if size > limit {
            anyhow::bail!("image exceeds MACHINA_METAL_IMAGE_MAX_BYTES ({limit})");
        }
        h.update(&chunk);
        f.write_all(&chunk).await?;
    }
    f.flush().await?;
    Ok((h.finish(), size))
}

async fn run_import(
    pool: &DbPool,
    id: Uuid,
    dir: &Path,
    rel: &str,
    p: &Plan,
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()?;

    let expected = match &p.expected {
        Expected::Hex(h) => h.clone(),
        Expected::FromSums { url, file } => {
            let text = client
                .get(url)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?;
            parse_sums(&text, file)
                .ok_or_else(|| anyhow::anyhow!("{file} is not listed in {url}"))?
        }
    };
    if expected.len() != p.algo.hex_len() || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("checksum is not a {} hex digest", p.algo.as_str());
    }

    tokio::fs::create_dir_all(dir).await?;
    let part = dir.join(format!("{rel}.part"));
    let limit = max_bytes();
    // A mirror can drop a long download. Retry the transfer a few times with a pause; a
    // checksum mismatch below is not retried (the bytes arrived, they are wrong).
    const ATTEMPTS: u32 = 3;
    let mut attempt = 0;
    let (got, size) = loop {
        attempt += 1;
        match download(&client, &p.url, &part, p.algo, limit).await {
            Ok(v) => break v,
            Err(e)
                if attempt < ATTEMPTS
                    && !e.to_string().contains("MACHINA_METAL_IMAGE_MAX_BYTES") =>
            {
                tracing::warn!("image {rel}: attempt {attempt} failed: {e:#}; retrying");
                tokio::time::sleep(retry_pause(attempt)).await;
            }
            Err(e) => return Err(e),
        }
    };
    if got != expected {
        let _ = tokio::fs::remove_file(&part).await;
        anyhow::bail!(
            "{} mismatch: expected {expected}, downloaded file is {got}",
            p.algo.as_str()
        );
    }
    tokio::fs::rename(&part, dir.join(rel)).await?;
    crate::db::query(
        "UPDATE baremetal_images SET status = 'ready', error = '', size_bytes = ?, checksum = ?
         WHERE id = ?",
    )
    .bind(size as i64)
    .bind(&got)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete(pool: &DbPool, name: &str) -> anyhow::Result<()> {
    let row: Option<String> =
        crate::db::query_scalar("SELECT path FROM baremetal_images WHERE name = ?")
            .bind(name)
            .fetch_optional(pool)
            .await?;
    let path = row.ok_or_else(|| anyhow::anyhow!("image not found"))?;
    crate::db::query("DELETE FROM baremetal_images WHERE name = ?")
        .bind(name)
        .execute(pool)
        .await?;
    if safe_file_component(&path) {
        let _ = tokio::fs::remove_file(images_dir().join(path)).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gnu_and_bsd_checksum_files() {
        let gnu = "aa11 *a.img\nbb22  b.img\n# c\n";
        assert_eq!(parse_sums(gnu, "a.img").as_deref(), Some("aa11"));
        assert_eq!(parse_sums(gnu, "b.img").as_deref(), Some("bb22"));
        assert_eq!(parse_sums(gnu, "c.img"), None);
        let bsd = "# Rocky: 1 bytes\nSHA256 (r.qcow2) = DEADbeef\nSHA256 (r.qcow2.sig) = 0000\n";
        assert_eq!(parse_sums(bsd, "r.qcow2").as_deref(), Some("deadbeef"));
        assert_eq!(parse_sums(bsd, "other"), None);
    }

    #[test]
    fn catalog_is_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for c in CATALOG {
            assert!(seen.insert(c.name), "duplicate {}", c.name);
            assert!(valid_name(c.name));
            assert!(c.url.starts_with("https://") && c.sums_url.starts_with("https://"));
            assert!(!url_file_name(c.url).is_empty());
        }
        assert!(catalog_entry("ubuntu-26.04").is_some());
        assert!(catalog_entry("windows-11").is_none());
    }

    #[test]
    fn requests_are_validated() {
        let r = |name: &str, url: &str, sum: &str| ImportRequest {
            name: name.into(),
            url: url.into(),
            checksum: sum.into(),
            checksum_algo: String::new(),
            sums_url: String::new(),
            os: String::new(),
            arch: String::new(),
            format: String::new(),
        };
        assert!(plan(&r("ubuntu-26.04", "", "")).is_ok());
        assert!(plan(&r("Bad Name", "", "")).is_err());
        assert!(plan(&r("custom", "", "")).is_err());
        assert!(plan(&r("custom", "ftp://x/a.img", "ab")).is_err());
        // A URL import is refused without any checksum source.
        assert!(plan(&r("custom", "https://m.local/a.img", "")).is_err());
        assert!(plan(&r("custom", "https://m.local/a.img", &"a".repeat(64))).is_ok());
    }

    async fn serve_once(body: &'static [u8]) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..2 {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(body).await;
            }
        });
        format!("http://{addr}/disk.img")
    }

    /// First connection promises the whole body and drops after half of it; the second is whole.
    async fn serve_flaky(body: &'static [u8]) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            for n in 0..2 {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes()).await;
                let part = if n == 0 { body.len() / 2 } else { body.len() };
                let _ = s.write_all(&body[..part]).await;
            }
        });
        format!("http://{addr}/disk.img")
    }

    #[tokio::test]
    async fn a_dropped_transfer_is_retried_and_still_verified() {
        let pool = crate::db::testing::pool().await;
        let dir = std::env::temp_dir().join(format!("machina-img-{}", Uuid::new_v4().simple()));
        let body: &'static [u8] = b"0123456789 a long enough body to be cut in half by the server";
        let good = hex::encode(Sha256::digest(body));
        let url = serve_flaky(body).await;
        import_to(&pool, &dir, url, good).await.unwrap();
        assert_eq!(std::fs::read(dir.join("t-disk.img")).unwrap(), body);
        std::fs::remove_dir_all(&dir).ok();
    }

    async fn import_to(
        pool: &DbPool,
        dir: &Path,
        url: String,
        checksum: String,
    ) -> anyhow::Result<()> {
        let id = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO baremetal_images (id, name, status) VALUES (?, ?, 'downloading')",
        )
        .bind(id)
        .bind(format!("t-{}", id.simple()))
        .execute(pool)
        .await?;
        let p = Plan {
            url,
            expected: Expected::Hex(checksum),
            algo: Algo::Sha256,
            file_name: "disk.img".into(),
            format: "raw".into(),
            os: String::new(),
            arch: "amd64".into(),
        };
        run_import(pool, id, dir, "t-disk.img", &p).await
    }

    #[tokio::test]
    async fn a_download_is_kept_only_when_the_checksum_matches() {
        let pool = crate::db::testing::pool().await;
        let dir = std::env::temp_dir().join(format!("machina-img-{}", Uuid::new_v4().simple()));
        let body: &'static [u8] = b"not really a disk image";
        let good = hex::encode(Sha256::digest(body));

        let url = serve_once(body).await;
        let bad = import_to(&pool, &dir, url.clone(), "0".repeat(64)).await;
        assert!(bad.unwrap_err().to_string().contains("mismatch"));
        assert!(!dir.join("t-disk.img").exists());
        assert!(!dir.join("t-disk.img.part").exists());

        import_to(&pool, &dir, url, good).await.unwrap();
        assert_eq!(std::fs::read(dir.join("t-disk.img")).unwrap(), body);
        std::fs::remove_dir_all(&dir).ok();
    }
}
