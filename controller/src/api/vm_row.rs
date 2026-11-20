// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use uuid::Uuid;

use crate::api::ApiError;
use crate::state::AppState;

/// VM name, optional host id, inventory source (`libvirt`, `kubevirt`, …).
pub async fn vm_inventory_row(
    state: &AppState,
    vm_id: Uuid,
) -> Result<(String, Option<Uuid>, String), ApiError> {
    crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| ApiError::not_found("VM not found"))
}

pub fn require_libvirt_inventory(source: &str) -> Result<(), ApiError> {
    if source == "kubevirt" {
        return Err(ApiError::bad_request(
            "Libvirt operations apply to libvirt-managed VMs only",
        ));
    }
    Ok(())
}

/// Resolve libvirt domain name + host id for agent RPC (rejects KubeVirt inventory).
pub async fn vm_agent_row_libvirt(
    state: &AppState,
    vm_id: Uuid,
) -> Result<(String, Uuid), ApiError> {
    let row = vm_inventory_row(state, vm_id).await?;
    require_libvirt_inventory(&row.2)?;
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("VM has no host assigned"))?;
    Ok((row.0, host_id))
}
