// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use sha2::{Digest, Sha256};
use crate::db::DbPool;
use uuid::Uuid;

use machina_core::FirewallInventory;

#[derive(Debug, Clone, Serialize)]
pub struct DriftReport {
    pub drift_detected: bool,
    pub expected: String,
    pub actual: String,
    pub summary: String,
}

fn drift_relevant_inventory(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = value.as_object_mut() {
        // Runtime sockets, activity counters, and the derived score change as
        // workloads come and go. They are observability data, not firewall
        // policy, and must not trigger "changed outside Zeus" drift events.
        obj.remove("hostname");
        obj.remove("open_ports");
        obj.remove("services");
        obj.remove("activity");
        obj.remove("score");
        if let Some(posture) = obj.get_mut("posture").and_then(|v| v.as_object_mut()) {
            posture.remove("status_line");
            posture.remove("drift_detected");
            posture.remove("last_changed");
        }
    }
    value
}

pub fn checksum_inventory(inv: &FirewallInventory) -> String {
    let relevant = serde_json::to_value(inv)
        .map(drift_relevant_inventory)
        .unwrap_or_default();
    let json = serde_json::to_string(&relevant).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
}

pub async fn save_snapshot(
    pool: &DbPool,
    target_kind: &str,
    target_id: Uuid,
    inv: &FirewallInventory,
) -> anyhow::Result<()> {
    let checksum = checksum_inventory(inv);
    let unchanged = crate::db::query_scalar::<_, String>(
        "SELECT checksum FROM firewall_posture_snapshots
         WHERE target_kind = ? AND target_id = ? ORDER BY captured_at DESC LIMIT 1",
    )
    .bind(target_kind)
    .bind(target_id)
    .fetch_optional(pool)
    .await?
    .as_deref()
        == Some(checksum.as_str());
    if unchanged {
        return Ok(());
    }
    crate::db::query(
        "INSERT INTO firewall_posture_snapshots (id, target_kind, target_id, checksum, posture_json) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(target_kind)
    .bind(target_id)
    .bind(&checksum)
    .bind(serde_json::to_value(inv)?)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn detect_drift(
    pool: &DbPool,
    target_kind: &str,
    target_id: Uuid,
    current: &FirewallInventory,
) -> anyhow::Result<DriftReport> {
    let row: Option<(String, serde_json::Value)> = crate::db::query_as(
        "SELECT checksum, posture_json FROM firewall_posture_snapshots
         WHERE target_kind = ? AND target_id = ? ORDER BY captured_at DESC LIMIT 1",
    )
    .bind(target_kind)
    .bind(target_id)
    .fetch_optional(pool)
    .await?;

    let current_sum = checksum_inventory(current);
    let Some((expected_sum, expected_json)) = row else {
        return Ok(DriftReport {
            drift_detected: false,
            expected: "no baseline".into(),
            actual: current_sum,
            summary: "No baseline snapshot yet".into(),
        });
    };

    if expected_sum == current_sum {
        return Ok(DriftReport {
            drift_detected: false,
            expected: expected_sum,
            actual: current_sum,
            summary: "Firewall matches Zeus baseline".into(),
        });
    }

    let expected_rules = expected_json
        .get("rules")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let actual_rules = current.rules.len();
    Ok(DriftReport {
        drift_detected: true,
        expected: format!("{expected_rules} rules (checksum {expected_sum})"),
        actual: format!("{actual_rules} rules (checksum {current_sum})"),
        summary: "Firewall changed outside Zeus OS".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::drift_relevant_inventory;
    use serde_json::json;

    #[test]
    fn ignores_runtime_observability_fields_for_drift() {
        let base = json!({
            "hostname": "host-a",
            "posture": {"enabled": true, "last_changed": "before", "status_line": "old"},
            "rules": [{"chain": "input"}],
            "services": [{"port": 22}],
            "profiles_available": ["server"],
            "open_ports": [{"port": 22}],
            "activity": {"accepted": 1},
            "score": {"value": 90}
        });
        let changed_runtime = json!({
            "hostname": "renamed-host",
            "posture": {"enabled": true, "last_changed": "after", "status_line": "new"},
            "rules": [{"chain": "input"}],
            "services": [{"port": 443}],
            "profiles_available": ["server"],
            "open_ports": [{"port": 22}, {"port": 443}],
            "activity": {"accepted": 999},
            "score": {"value": 80}
        });

        assert_eq!(
            drift_relevant_inventory(base),
            drift_relevant_inventory(changed_runtime)
        );
    }

    #[test]
    fn preserves_policy_fields_for_drift() {
        let expected = drift_relevant_inventory(json!({
            "posture": {"enabled": true},
            "rules": [{"chain": "input"}]
        }));
        let changed = drift_relevant_inventory(json!({
            "posture": {"enabled": true},
            "rules": [{"chain": "forward"}]
        }));

        assert_ne!(expected, changed);
    }
}
