// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Download bundled marketplace golden images to hypervisors when missing.

use std::path::Path;
use std::time::Duration;

use crate::db::DbPool;
use uuid::Uuid;

use super::host_shell;
use super::template_catalog;
use super::template_readiness;

const FETCH_TIMEOUT_SECS: u64 = 3600;

pub async fn ensure_template_disk(
    pool: &DbPool,
    host_id: Uuid,
    dest_path: &str,
    template_name: &str,
    template_version: &str,
) -> anyhow::Result<bool> {
    if template_readiness::disk_exists_at(dest_path).await
        || template_readiness::disk_exists_on_hosts(pool, dest_path).await
    {
        return Ok(false);
    }

    let Some(url) = template_catalog::download_url_for(template_name, template_version) else {
        anyhow::bail!(
            "Golden image missing at {dest_path} — no automatic download for this template. Upload via Content Library."
        );
    };

    let addr = host_address(pool, host_id).await?;
    fetch_to_host(&addr, dest_path, url).await?;
    Ok(true)
}

async fn host_address(pool: &DbPool, host_id: Uuid) -> anyhow::Result<String> {
    let row: Option<(String, String)> =
        crate::db::query_as("SELECT hostname, address FROM hosts WHERE id = ? AND state = 'online'")
            .bind(host_id)
            .fetch_optional(pool)
            .await?;
    let (hostname, address) = row.ok_or_else(|| anyhow::anyhow!("host not online"))?;
    let addr = if address.trim().is_empty() {
        hostname
    } else {
        address
    };
    Ok(addr)
}

async fn fetch_to_host(address: &str, dest_path: &str, url: &str) -> anyhow::Result<()> {
    let parent = Path::new(dest_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/var/lib/libvirt/images".into());
    let tmp = format!("{dest_path}.machina-fetch.part");
    let script = format!(
        r#"set -euo pipefail
mkdir -p '{parent}'
if [ -f '{dest}' ]; then exit 0; fi
command -v curl >/dev/null || command -v wget >/dev/null || {{ echo 'curl or wget required'; exit 1; }}
rm -f '{tmp}'
if command -v curl >/dev/null; then
  curl -fL --retry 3 --retry-delay 5 -o '{tmp}' '{url}'
else
  wget -q -O '{tmp}' '{url}'
fi
if echo '{dest}' | grep -q '\.qcow2$' && echo '{url}' | grep -qE '\.(img|raw)$'; then
  qemu-img convert -O qcow2 '{tmp}' '{dest}'
  rm -f '{tmp}'
else
  mv '{tmp}' '{dest}'
fi
chmod 644 '{dest}'
"#,
        parent = parent.replace('\'', "'\\''"),
        dest = dest_path.replace('\'', "'\\''"),
        tmp = tmp.replace('\'', "'\\''"),
        url = url.replace('\'', "'\\''"),
    );

    let out = match host_shell::run_remote_script(
        address,
        Duration::from_secs(FETCH_TIMEOUT_SECS),
        &script,
    )
    .await
    {
        Ok(o) => o,
        Err(e) if e.to_string().contains("timed out") => {
            anyhow::bail!("template image download timed out after {FETCH_TIMEOUT_SECS}s")
        }
        Err(e) => anyhow::bail!("{e}"),
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        anyhow::bail!("template image download failed: {stderr}{stdout}");
    }
    Ok(())
}

/// Download all missing auto-fetch marketplace images onto one online host.
pub async fn prefetch_missing_images(
    pool: &DbPool,
    host_id: Option<Uuid>,
) -> anyhow::Result<(usize, usize, Vec<String>)> {
    let host_id = match host_id {
        Some(id) => id,
        None => {
            let id: Option<Uuid> = crate::db::query_scalar(
                "SELECT id FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 1",
            )
            .fetch_optional(pool)
            .await?;
            id.ok_or_else(|| anyhow::anyhow!("no online hosts to download images"))?
        }
    };

    let missing = template_readiness::list_missing_marketplace_images(pool).await?;
    let mut fetched = 0usize;
    let mut skipped = 0usize;
    let mut errors = Vec::new();

    for item in missing {
        if !item.auto_fetch {
            skipped += 1;
            continue;
        }
        match ensure_template_disk(pool, host_id, &item.source_disk, &item.name, &item.version)
            .await
        {
            Ok(true) => fetched += 1,
            Ok(false) => skipped += 1,
            Err(e) => errors.push(format!("{}@{}: {e:#}", item.name, item.version)),
        }
    }

    Ok((fetched, skipped, errors))
}
