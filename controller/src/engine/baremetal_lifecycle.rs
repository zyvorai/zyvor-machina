// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Bare-metal lifecycle and commissioning.
//!
//! `new -> commissioning -> ready -> allocated -> deploying -> deployed -> retired`,
//! plus `failed`. A transition is a compare-and-set on the stored state, so two
//! callers cannot both win. Commissioning hands the machine a one-time token (only
//! its SHA-256 is stored); the machine returns its hardware report with it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::db::DbPool;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    New,
    Commissioning,
    Ready,
    Allocated,
    Deploying,
    Deployed,
    Retired,
    Failed,
}

impl Lifecycle {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "new" => Self::New,
            "commissioning" => Self::Commissioning,
            "ready" => Self::Ready,
            "allocated" => Self::Allocated,
            "deploying" => Self::Deploying,
            "deployed" => Self::Deployed,
            "retired" => Self::Retired,
            "failed" => Self::Failed,
            other => anyhow::bail!("unknown lifecycle state '{other}'"),
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Commissioning => "commissioning",
            Self::Ready => "ready",
            Self::Allocated => "allocated",
            Self::Deploying => "deploying",
            Self::Deployed => "deployed",
            Self::Retired => "retired",
            Self::Failed => "failed",
        }
    }

    pub fn can_transition_to(self, to: Self) -> bool {
        use Lifecycle::*;
        matches!(
            (self, to),
            (New, Commissioning)
                | (Commissioning, Ready)
                | (Commissioning, Failed)
                | (Commissioning, New)
                | (Ready, Allocated)
                | (Ready, Commissioning)
                | (Ready, Retired)
                | (Allocated, Deploying)
                | (Allocated, Ready)
                | (Deploying, Deployed)
                | (Deploying, Failed)
                | (Deployed, Ready)
                | (Deployed, Retired)
                | (Failed, Commissioning)
                | (Failed, New)
                | (Failed, Retired)
                | (Retired, New)
        )
    }
}

async fn current(pool: &DbPool, id: Uuid) -> anyhow::Result<Lifecycle> {
    let s: String = crate::db::query_scalar("SELECT lifecycle FROM baremetal_servers WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("server not found"))?;
    Lifecycle::parse(&s)
}

/// Move a server to `to`, refusing a transition the state machine does not allow.
pub async fn transition(pool: &DbPool, id: Uuid, to: Lifecycle) -> anyhow::Result<Lifecycle> {
    let from = current(pool, id).await?;
    if !from.can_transition_to(to) {
        anyhow::bail!(
            "cannot move a server from {} to {}",
            from.as_str(),
            to.as_str()
        );
    }
    let res = crate::db::query(
        "UPDATE baremetal_servers SET lifecycle = ?, last_error = '' WHERE id = ? AND lifecycle = ?",
    )
    .bind(to.as_str())
    .bind(id)
    .bind(from.as_str())
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        anyhow::bail!("server changed state concurrently; retry");
    }
    Ok(to)
}

pub fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn new_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// Start commissioning: `new|ready|failed -> commissioning` and issue the one-time token.
pub async fn begin_commissioning(pool: &DbPool, id: Uuid) -> anyhow::Result<String> {
    transition(pool, id, Lifecycle::Commissioning).await?;
    let token = new_token();
    crate::db::query("UPDATE baremetal_servers SET commission_token_hash = ? WHERE id = ?")
        .bind(token_hash(&token))
        .bind(id)
        .execute(pool)
        .await?;
    // The next boot must be PXE even if the machine has an OS on disk.
    let note = super::baremetal::boot_network_best_effort(pool, id).await;
    tracing::info!("commissioning {id}: {note}");
    Ok(token)
}

/// Totals read from a commissioning report. Missing fields count as zero.
#[derive(Debug, PartialEq, Eq)]
pub struct ReportSummary {
    pub cpu_cores: i64,
    pub memory_mib: i64,
    pub disks: i64,
    pub nics: i64,
    pub passed: bool,
}

pub fn summarize_report(report: &Value) -> ReportSummary {
    let n = |k: &str| report.get(k).and_then(Value::as_i64).unwrap_or(0).max(0);
    let len = |k: &str| {
        report
            .get(k)
            .and_then(Value::as_array)
            .map_or(0, |a| a.len() as i64)
    };
    let tests_ok = report
        .get("tests")
        .and_then(Value::as_array)
        .map(|t| {
            t.iter()
                .all(|x| x.get("passed").and_then(Value::as_bool).unwrap_or(false))
        })
        .unwrap_or(true);
    ReportSummary {
        cpu_cores: n("cpu_cores"),
        memory_mib: n("memory_mib"),
        disks: len("disks"),
        nics: len("nics"),
        passed: tests_ok,
    }
}

/// Accept a machine's report. The token must match the one issued by
/// `begin_commissioning`; it is cleared on use. Returns the resulting state.
pub async fn accept_report(
    pool: &DbPool,
    id: Uuid,
    token: &str,
    report: &Value,
) -> anyhow::Result<Lifecycle> {
    let (state, hash): (String, String) = crate::db::query_as(
        "SELECT lifecycle, commission_token_hash FROM baremetal_servers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("server not found"))?;
    if Lifecycle::parse(&state)? != Lifecycle::Commissioning
        || hash.is_empty()
        || hash != token_hash(token)
    {
        // One message for every cause: do not tell a caller which part was wrong.
        anyhow::bail!("invalid or expired commissioning token");
    }
    let s = summarize_report(report);
    let to = if s.passed {
        Lifecycle::Ready
    } else {
        Lifecycle::Failed
    };
    crate::db::query(
        "INSERT INTO baremetal_inventory
         (server_id, report_json, cpu_cores, memory_mib, disk_count, nic_count)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (server_id) DO UPDATE SET report_json = excluded.report_json,
           cpu_cores = excluded.cpu_cores, memory_mib = excluded.memory_mib,
           disk_count = excluded.disk_count, nic_count = excluded.nic_count,
           collected_at = CURRENT_TIMESTAMP",
    )
    .bind(id)
    .bind(report.to_string())
    .bind(s.cpu_cores)
    .bind(s.memory_mib)
    .bind(s.disks)
    .bind(s.nics)
    .execute(pool)
    .await?;
    let res = crate::db::query(
        "UPDATE baremetal_servers
         SET lifecycle = ?, commission_token_hash = '', cpu_cores = ?, memory_mib = ?,
             last_error = ?
         WHERE id = ? AND lifecycle = 'commissioning'",
    )
    .bind(to.as_str())
    .bind(s.cpu_cores)
    .bind(s.memory_mib)
    .bind(if s.passed {
        ""
    } else {
        "commissioning tests failed"
    })
    .bind(id)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        anyhow::bail!("invalid or expired commissioning token");
    }
    Ok(to)
}

pub fn auto_enroll_enabled() -> bool {
    matches!(
        std::env::var("MACHINA_METAL_AUTO_ENROLL").ok().as_deref(),
        Some("1" | "true" | "yes")
    )
}

/// An unknown MAC PXE-booted with auto-enroll on: record it as `new` (named from the MAC,
/// no BMC details yet) and start commissioning. At most one row per MAC, even when two
/// requests race. Returns the new server's id, or `None` if the MAC is already known.
pub async fn auto_enroll(pool: &DbPool, mac: &str) -> anyhow::Result<Option<Uuid>> {
    let Some(mac) = super::baremetal_boot::normalize_mac(mac) else {
        return Ok(None);
    };
    let id = Uuid::new_v4();
    let host = format!("metal-{}", mac.replace(':', ""));
    let res = crate::db::query(
        "INSERT INTO baremetal_servers (id, hostname, bmc_type, state, mac_address)
         SELECT ?, ?, 'redfish', 'discovered', ?
         WHERE NOT EXISTS (SELECT 1 FROM baremetal_servers WHERE mac_address = ?)",
    )
    .bind(id)
    .bind(host)
    .bind(&mac)
    .bind(&mac)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Ok(None);
    }
    begin_commissioning(pool, id).await?;
    Ok(Some(id))
}

/// A PXE-booting machine asks what to boot. Only a server that is being commissioned and
/// whose MAC is registered gets an image; it receives a fresh one-time token (the issued
/// one is stored hashed, so it cannot be put in a boot script). Anything else boots from disk.
pub async fn boot_target(pool: &DbPool, mac: &str) -> anyhow::Result<Option<(Uuid, String)>> {
    let Some(mac) = super::baremetal_boot::normalize_mac(mac) else {
        return Ok(None);
    };
    let mut id: Option<Uuid> = crate::db::query_scalar(
        "SELECT id FROM baremetal_servers WHERE mac_address = ? AND lifecycle = 'commissioning'",
    )
    .bind(&mac)
    .fetch_optional(pool)
    .await?;
    if id.is_none() && auto_enroll_enabled() {
        id = auto_enroll(pool, &mac).await?;
    }
    let Some(id) = id else { return Ok(None) };
    let token = new_token();
    crate::db::query(
        "UPDATE baremetal_servers SET commission_token_hash = ?
         WHERE id = ? AND lifecycle = 'commissioning'",
    )
    .bind(token_hash(&token))
    .bind(id)
    .execute(pool)
    .await?;
    Ok(Some((id, token)))
}

pub async fn inventory(pool: &DbPool, id: Uuid) -> anyhow::Result<Option<Value>> {
    let row: Option<(String,)> =
        crate::db::query_as("SELECT report_json FROM baremetal_inventory WHERE server_id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(j,)| serde_json::from_str(&j).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn server(pool: &DbPool) -> Uuid {
        let id = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO baremetal_servers (id, hostname, bmc_type) VALUES (?, ?, 'fake')",
        )
        .bind(id)
        .bind(format!("m-{}", id.simple()))
        .execute(pool)
        .await
        .unwrap();
        id
    }

    #[test]
    fn state_machine_allows_only_the_documented_edges() {
        use Lifecycle::*;
        assert!(New.can_transition_to(Commissioning));
        assert!(Ready.can_transition_to(Allocated));
        assert!(Deployed.can_transition_to(Ready));
        assert!(!New.can_transition_to(Deployed));
        assert!(!Ready.can_transition_to(Deploying));
        assert!(!Retired.can_transition_to(Ready));
        assert!(!Allocated.can_transition_to(Allocated));
        assert!(Lifecycle::parse("bogus").is_err());
    }

    #[test]
    fn report_summary_counts_and_test_verdict() {
        let r = json!({"cpu_cores": 32, "memory_mib": 262144,
            "disks": [{"name":"sda"},{"name":"sdb"}], "nics": [{"name":"eno1"}],
            "tests": [{"name":"mem","passed":true},{"name":"smart","passed":false}]});
        let s = summarize_report(&r);
        assert_eq!(
            (s.cpu_cores, s.memory_mib, s.disks, s.nics),
            (32, 262144, 2, 1)
        );
        assert!(!s.passed);
        assert!(summarize_report(&json!({})).passed);
    }

    #[tokio::test]
    async fn commissioning_round_trip_and_token_is_single_use() {
        let pool = crate::db::testing::pool().await;
        let id = server(&pool).await;
        let token = begin_commissioning(&pool, id).await.unwrap();
        assert!(accept_report(&pool, id, "wrong", &json!({})).await.is_err());
        let to = accept_report(
            &pool,
            id,
            &token,
            &json!({"cpu_cores": 8, "memory_mib": 16384, "disks": [{}], "nics": [{}, {}]}),
        )
        .await
        .unwrap();
        assert_eq!(to, Lifecycle::Ready);
        assert!(accept_report(&pool, id, &token, &json!({})).await.is_err());
        let inv = inventory(&pool, id).await.unwrap().unwrap();
        assert_eq!(inv["cpu_cores"], 8);
        let cores: i64 =
            crate::db::query_scalar("SELECT cpu_cores FROM baremetal_servers WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cores, 8);
    }

    #[tokio::test]
    async fn boot_target_serves_only_a_commissioning_machine_by_mac() {
        let pool = crate::db::testing::pool().await;
        let id = server(&pool).await;
        crate::db::query(
            "UPDATE baremetal_servers SET mac_address = 'aa:bb:cc:dd:ee:ff' WHERE id = ?",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        assert!(boot_target(&pool, "AA-BB-CC-DD-EE-FF")
            .await
            .unwrap()
            .is_none());
        begin_commissioning(&pool, id).await.unwrap();
        let (got, token) = boot_target(&pool, "AA-BB-CC-DD-EE-FF")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got, id);
        assert!(boot_target(&pool, "11:22:33:44:55:66")
            .await
            .unwrap()
            .is_none());
        assert!(boot_target(&pool, "nonsense").await.unwrap().is_none());
        // The minted token is the one that now works.
        let r = accept_report(&pool, id, &token, &json!({})).await.unwrap();
        assert_eq!(r, Lifecycle::Ready);
        assert!(boot_target(&pool, "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn auto_enroll_creates_one_commissioning_row_per_mac() {
        let pool = crate::db::testing::pool().await;
        let id = auto_enroll(&pool, "AA-BB-CC-DD-EE-01")
            .await
            .unwrap()
            .unwrap();
        assert!(auto_enroll(&pool, "aa:bb:cc:dd:ee:01")
            .await
            .unwrap()
            .is_none());
        assert!(auto_enroll(&pool, "garbage").await.unwrap().is_none());
        let (host, lc, mac): (String, String, String) = crate::db::query_as(
            "SELECT hostname, lifecycle, mac_address FROM baremetal_servers WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(host, "metal-aabbccddee01");
        assert_eq!(lc, "commissioning");
        assert_eq!(mac, "aa:bb:cc:dd:ee:01");
        let n: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM baremetal_servers")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn failed_tests_land_in_failed_and_illegal_transition_is_refused() {
        let pool = crate::db::testing::pool().await;
        let id = server(&pool).await;
        assert!(transition(&pool, id, Lifecycle::Deployed).await.is_err());
        let token = begin_commissioning(&pool, id).await.unwrap();
        let to = accept_report(
            &pool,
            id,
            &token,
            &json!({"tests": [{"name": "mem", "passed": false}]}),
        )
        .await
        .unwrap();
        assert_eq!(to, Lifecycle::Failed);
        assert!(begin_commissioning(&pool, id).await.is_ok());
    }
}
