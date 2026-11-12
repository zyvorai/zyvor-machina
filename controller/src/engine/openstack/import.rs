// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::path::PathBuf;

use uuid::Uuid;

use crate::state::AppState;

use super::client::{ConnectionConfig, OpenStackClient};
use super::inventory::InventoryKind;

#[derive(Debug, Clone)]
pub struct ImportRequest {
    pub connection_id: String,
    pub kind: InventoryKind,
    pub remote_id: String,
    pub dest_name: String,
    pub host_id: Option<Uuid>,
    /// When set (mock/lab), write bytes here instead of SSHing to a host.
    pub local_stage_dir: Option<PathBuf>,
}

pub async fn run_import_job(state: &AppState, job_id: &str, cfg: &ConnectionConfig, req: &ImportRequest) -> anyhow::Result<Uuid> {
    set_job(state, job_id, "downloading", 10, "").await?;
    if req.kind != InventoryKind::Image {
        anyhow::bail!("only Glance image imports are supported in v1 (create a Glance image from the Cinder volume first)");
    }
    let client = OpenStackClient::connect(cfg).await?;
    if client.image.is_empty() {
        anyhow::bail!("no Glance endpoint in Keystone catalog");
    }
    let file_url = format!("{}/v2/images/{}/file", client.image, req.remote_id);
    let bytes = client.download_bytes(&file_url).await?;
    set_job(state, job_id, "importing", 60, "").await?;

    let dest_name = if req.dest_name.trim().is_empty() {
        format!("os-import-{}", &req.remote_id[..8.min(req.remote_id.len())])
    } else {
        req.dest_name.clone()
    };
    let staged = if let Some(dir) = &req.local_stage_dir {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{dest_name}.qcow2"));
        std::fs::write(&path, &bytes)?;
        path.to_string_lossy().to_string()
    } else {
        let host_id = match req.host_id {
            Some(h) => h,
            None => crate::db::query_scalar("SELECT id FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 1")
                .fetch_optional(&state.pool)
                .await?
                .ok_or_else(|| anyhow::anyhow!("no online host for staging"))?,
        };
        let dest = format!("/var/lib/libvirt/images/{dest_name}.qcow2");
        stage_on_host(state, host_id, &dest, &file_url, client.token()).await?;
        dest
    };

    let template_id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO templates (id, name, version, source_disk, cloud_init, os_family, category, description, featured, marketplace) \
         VALUES (?, ?, '1', ?, TRUE, 'linux', 'Linux', 'Imported from OpenStack', FALSE, FALSE)",
    )
    .bind(template_id)
    .bind(&dest_name)
    .bind(&staged)
    .execute(&state.pool)
    .await?;

    crate::db::query(
        "UPDATE openstack_import_jobs SET status = 'done', progress = 100, staged_path = ?, template_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(&staged)
    .bind(template_id.to_string())
    .bind(job_id)
    .execute(&state.pool)
    .await?;
    Ok(template_id)
}

async fn set_job(state: &AppState, id: &str, status: &str, progress: i64, error: &str) -> anyhow::Result<()> {
    crate::db::query(
        "UPDATE openstack_import_jobs SET status = ?, progress = ?, error = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(status)
    .bind(progress)
    .bind(error)
    .bind(id)
    .execute(&state.pool)
    .await?;
    Ok(())
}

async fn stage_on_host(
    state: &AppState,
    host_id: Uuid,
    dest: &str,
    url: &str,
    token: &str,
) -> anyhow::Result<()> {
    let row: Option<(String, String)> =
        crate::db::query_as("SELECT hostname, address FROM hosts WHERE id = ? AND state = 'online'")
            .bind(host_id)
            .fetch_optional(&state.pool)
            .await?;
    let (hostname, address) = row.ok_or_else(|| anyhow::anyhow!("host not online"))?;
    let addr = if address.trim().is_empty() {
        hostname
    } else {
        address
    };
    let parent = std::path::Path::new(dest)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/var/lib/libvirt/images".into());
    let tmp = format!("{dest}.machina-os-import.part");
    let script = format!(
        r#"set -euo pipefail
mkdir -p '{parent}'
rm -f '{tmp}'
curl -fL --retry 3 -H 'X-Auth-Token: {token}' -o '{tmp}' '{url}'
mv '{tmp}' '{dest}'
chmod 644 '{dest}'
"#
    );
    crate::engine::host_shell::run_remote_script(&addr, std::time::Duration::from_secs(3600), &script)
        .await?;
    let _ = state;
    Ok(())
}
