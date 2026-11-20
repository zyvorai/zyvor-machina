// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::collections::HashSet;

use crate::db::DbPool;
use uuid::Uuid;

use crate::state::AppState;

const DOMAIN_MISSING: &str = "domain_not_found: VM no longer present on hypervisor inventory scan";

#[derive(Debug, sqlx::FromRow)]
pub struct ClusterInventoryPolicy {
    pub inventory_prune_unmanaged: bool,
    pub inventory_mark_managed_missing: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct VmInventoryRow {
    id: Uuid,
    name: String,
    managed: bool,
    lifecycle_phase: String,
}

/// Transient phases during which a task is actively creating/removing/moving the domain
/// on the host. An inventory scan that races such a task must NOT mark the VM 'missing'
/// or prune it — the domain may not be defined on the host yet (e.g. `vm.apply` is still
/// downloading a template). Clobbering it here caused a just-created VM to flip to
/// 'missing' and even let a delete orphan the domain being created.
const IN_FLIGHT_PHASES: &[&str] = &["creating", "deleting", "migrating"];

pub async fn cluster_inventory_policy(
    pool: &DbPool,
    cluster_id: Uuid,
) -> anyhow::Result<ClusterInventoryPolicy> {
    let row = crate::db::query_as::<_, ClusterInventoryPolicy>(
        "SELECT inventory_prune_unmanaged, inventory_mark_managed_missing FROM clusters WHERE id = ?",
    )
    .bind(cluster_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.unwrap_or(ClusterInventoryPolicy {
        inventory_prune_unmanaged: true,
        inventory_mark_managed_missing: true,
    }))
}

/// Remove or mark libvirt VMs on `host_id` that were not seen in the latest agent inventory.
pub async fn reconcile_libvirt_host(
    state: &AppState,
    host_id: Uuid,
    cluster_id: Uuid,
    seen_names: &HashSet<String>,
) -> anyhow::Result<()> {
    let policy = cluster_inventory_policy(&state.pool, cluster_id).await?;
    let rows: Vec<VmInventoryRow> = crate::db::query_as(
        "SELECT id, name, managed, lifecycle_phase FROM vms
         WHERE host_id = ? AND inventory_source = 'libvirt'",
    )
    .bind(host_id)
    .fetch_all(&state.pool)
    .await?;

    for row in rows {
        if seen_names.contains(&row.name) {
            continue;
        }
        // Don't reconcile a VM that a lifecycle task is mid-flight on — the domain may
        // legitimately not be on the host yet. Prevents the create→apply race where a
        // just-created VM is flipped to 'missing' (and could then be delete-orphaned).
        if IN_FLIGHT_PHASES.contains(&row.lifecycle_phase.as_str()) {
            continue;
        }
        if !row.managed && policy.inventory_prune_unmanaged {
            crate::db::query("DELETE FROM vms WHERE id = ?")
                .bind(row.id)
                .execute(&state.pool)
                .await?;
            state.emit_event(
                "vm.removed",
                format!(
                    "Discovered VM '{}' removed — absent from hypervisor",
                    row.name
                ),
            );
        } else if row.managed && policy.inventory_mark_managed_missing {
            crate::db::query(
                "UPDATE vms SET observed_state = 'missing', last_error = ?, updated_at = datetime('now') WHERE id = ?",
            )
            .bind(DOMAIN_MISSING)
            .bind(row.id)
            .execute(&state.pool)
            .await?;
            state.emit_event(
                "vm.missing",
                format!("VM '{}' missing from hypervisor inventory", row.name),
            );
        } else {
            tracing::debug!(
                vm = %row.name,
                host_id = %host_id,
                "inventory: VM absent from hypervisor but policy kept row"
            );
        }
    }
    Ok(())
}

/// A libvirt VM whose host is reporting inventory but which the host has not
/// listed for a while: its domain is gone and the row is stale.
#[derive(Debug, sqlx::FromRow)]
pub struct StaleVm {
    pub id: Uuid,
    pub name: String,
    pub managed: bool,
    pub last_seen: String,
}

const STALE_VMS_SQL: &str = "SELECT v.id, v.name, v.managed,
        COALESCE(v.last_seen_at, v.created_at) AS last_seen
     FROM vms v JOIN hosts h ON h.id = v.host_id
     WHERE v.inventory_source = 'libvirt'
       AND v.observed_state != 'missing'
       AND v.lifecycle_phase NOT IN ('creating', 'deleting', 'migrating')
       AND h.state = 'online'
       AND h.last_heartbeat_at > datetime('now', '-2 minutes')
       AND COALESCE(v.last_seen_at, v.created_at) < datetime('now', '-10 minutes')";

pub async fn stale_libvirt_vms(pool: &DbPool) -> anyhow::Result<Vec<StaleVm>> {
    Ok(
        crate::db::query_as::<_, StaleVm>(&format!("{STALE_VMS_SQL} ORDER BY v.name LIMIT 50"))
            .fetch_all(pool)
            .await?,
    )
}

pub async fn stale_libvirt_vm(pool: &DbPool, vm_id: Uuid) -> anyhow::Result<Option<StaleVm>> {
    Ok(
        crate::db::query_as::<_, StaleVm>(&format!("{STALE_VMS_SQL} AND v.id = ?"))
            .bind(vm_id)
            .fetch_optional(pool)
            .await?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_missing_message_stable() {
        assert!(DOMAIN_MISSING.contains("domain_not_found"));
    }
}
