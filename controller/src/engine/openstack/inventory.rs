// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::client::OpenStackClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InventoryKind {
    Server,
    Image,
    Volume,
}

impl InventoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Image => "image",
            Self::Volume => "volume",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryItem {
    pub kind: InventoryKind,
    pub remote_id: String,
    pub name: String,
    pub status: String,
    pub size_bytes: Option<i64>,
    pub disk_format: String,
    pub meta: Value,
}

pub async fn list_servers(client: &OpenStackClient) -> anyhow::Result<Vec<InventoryItem>> {
    if client.compute.is_empty() {
        return Ok(vec![]);
    }
    let v = client
        .get_json(&format!("{}/servers/detail", client.compute))
        .await?;
    let mut out = Vec::new();
    for s in v.get("servers").and_then(|x| x.as_array()).into_iter().flatten() {
        out.push(InventoryItem {
            kind: InventoryKind::Server,
            remote_id: s.get("id").and_then(|v| v.as_str()).unwrap_or("").into(),
            name: s.get("name").and_then(|v| v.as_str()).unwrap_or("").into(),
            status: s.get("status").and_then(|v| v.as_str()).unwrap_or("").into(),
            size_bytes: None,
            disk_format: String::new(),
            meta: s.clone(),
        });
    }
    Ok(out)
}

pub async fn list_images(client: &OpenStackClient) -> anyhow::Result<Vec<InventoryItem>> {
    if client.image.is_empty() {
        return Ok(vec![]);
    }
    let v = client.get_json(&format!("{}/v2/images", client.image)).await?;
    let mut out = Vec::new();
    for s in v.get("images").and_then(|x| x.as_array()).into_iter().flatten() {
        out.push(InventoryItem {
            kind: InventoryKind::Image,
            remote_id: s.get("id").and_then(|v| v.as_str()).unwrap_or("").into(),
            name: s.get("name").and_then(|v| v.as_str()).unwrap_or("").into(),
            status: s.get("status").and_then(|v| v.as_str()).unwrap_or("").into(),
            size_bytes: s.get("size").and_then(|v| v.as_i64()),
            disk_format: s
                .get("disk_format")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .into(),
            meta: s.clone(),
        });
    }
    Ok(out)
}

pub async fn list_volumes(client: &OpenStackClient) -> anyhow::Result<Vec<InventoryItem>> {
    if client.volume.is_empty() {
        return Ok(vec![]);
    }
    let v = client
        .get_json(&format!("{}/volumes/detail", client.volume))
        .await?;
    let mut out = Vec::new();
    for s in v.get("volumes").and_then(|x| x.as_array()).into_iter().flatten() {
        let size_gib = s.get("size").and_then(|v| v.as_i64()).unwrap_or(0);
        out.push(InventoryItem {
            kind: InventoryKind::Volume,
            remote_id: s.get("id").and_then(|v| v.as_str()).unwrap_or("").into(),
            name: s.get("name").and_then(|v| v.as_str()).unwrap_or("").into(),
            status: s.get("status").and_then(|v| v.as_str()).unwrap_or("").into(),
            size_bytes: Some(size_gib * 1024 * 1024 * 1024),
            disk_format: String::new(),
            meta: s.clone(),
        });
    }
    Ok(out)
}

pub async fn sync_inventory(
    pool: &crate::db::DbPool,
    connection_id: &str,
    client: &OpenStackClient,
) -> anyhow::Result<Vec<InventoryItem>> {
    let mut items = Vec::new();
    items.extend(list_servers(client).await?);
    items.extend(list_images(client).await?);
    items.extend(list_volumes(client).await?);

    crate::db::query("DELETE FROM openstack_inventory WHERE connection_id = ?")
        .bind(connection_id)
        .execute(pool)
        .await?;
    for item in &items {
        crate::db::query(
            "INSERT INTO openstack_inventory \
             (id, connection_id, kind, remote_id, name, status, size_bytes, disk_format, meta_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(connection_id)
        .bind(item.kind.as_str())
        .bind(&item.remote_id)
        .bind(&item.name)
        .bind(&item.status)
        .bind(item.size_bytes)
        .bind(&item.disk_format)
        .bind(item.meta.to_string())
        .execute(pool)
        .await?;
    }
    Ok(items)
}
