// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::db::DbPool;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::baremetal_bmc::{
    power, validate_address, BmcKind, BmcTarget, BootDevice, PowerAction, PowerState,
};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct BaremetalServer {
    pub id: Uuid,
    pub hostname: String,
    pub bmc_address: String,
    pub bmc_type: String,
    pub state: String,
    pub cpu_cores: i32,
    pub memory_mib: i64,
    pub firewall_profile: String,
    pub firewall_enabled: bool,
    pub bmc_vlan: String,
    pub pxe_vlan: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// A BMC password is stored (never returned).
    pub bmc_credentials_configured: bool,
    /// Outcome of the last failed live BMC call; empty when it succeeded.
    pub last_error: String,
    /// new, commissioning, ready, allocated, deploying, deployed, retired or failed.
    pub lifecycle: String,
    pub zone: String,
    pub mac_address: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterBaremetalBody {
    pub hostname: String,
    pub bmc_address: String,
    #[serde(default = "default_bmc_type")]
    pub bmc_type: String,
    #[serde(default)]
    pub cpu_cores: i32,
    #[serde(default)]
    pub memory_mib: i64,
    #[serde(default = "default_metal_profile")]
    pub firewall_profile: String,
    #[serde(default = "default_true")]
    pub firewall_enabled: bool,
    #[serde(default)]
    pub bmc_vlan: String,
    #[serde(default)]
    pub pxe_vlan: String,
    /// BMC login. The password is stored encrypted and is never returned.
    #[serde(default)]
    pub bmc_username: String,
    #[serde(default)]
    pub bmc_password: String,
    /// MAC of the NIC the machine PXE-boots from; matches it to this record.
    #[serde(default)]
    pub mac_address: String,
    #[serde(default)]
    pub zone: String,
}

fn default_bmc_type() -> String {
    "redfish".into()
}

fn default_metal_profile() -> String {
    "BareMetalBmc".into()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct BaremetalCapacityPlan {
    pub query: String,
    pub servers_needed: i32,
    pub total_cpu_cores: i32,
    pub total_memory_gib: i32,
    pub summary: String,
}

pub async fn list_servers(pool: &DbPool) -> anyhow::Result<Vec<BaremetalServer>> {
    let rows = crate::db::query_as::<_, BaremetalServer>(
        "SELECT id, hostname, bmc_address, bmc_type, state, cpu_cores, memory_mib,
                firewall_profile, firewall_enabled, bmc_vlan, pxe_vlan, created_at,
                (bmc_password_enc <> '') AS bmc_credentials_configured, last_error,
                lifecycle, zone, mac_address
         FROM baremetal_servers ORDER BY hostname",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn register(
    pool: &DbPool,
    body: &RegisterBaremetalBody,
) -> anyhow::Result<BaremetalServer> {
    let mac = if body.mac_address.trim().is_empty() {
        String::new()
    } else {
        super::baremetal_boot::normalize_mac(&body.mac_address)
            .ok_or_else(|| anyhow::anyhow!("mac_address is not a MAC address"))?
    };
    let kind = BmcKind::parse(&body.bmc_type)?;
    if kind != BmcKind::Fake && !body.bmc_address.trim().is_empty() {
        validate_address(&body.bmc_address)?;
    }
    if kind == BmcKind::Libvirt {
        super::baremetal_bmc::libvirt_name_allowed(body.bmc_address.trim())?;
    }
    let password_enc = if body.bmc_password.is_empty() {
        String::new()
    } else {
        crate::engine::ai::crypto::store_api_key(&body.bmc_password)?
    };
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO baremetal_servers
         (id, hostname, bmc_address, bmc_type, cpu_cores, memory_mib, state,
          firewall_profile, firewall_enabled, bmc_vlan, pxe_vlan,
          bmc_username, bmc_password_enc, mac_address, zone)
         VALUES (?, ?, ?, ?, ?, ?, 'registered', ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(body.hostname.trim())
    .bind(body.bmc_address.trim())
    .bind(&body.bmc_type)
    .bind(body.cpu_cores.max(0))
    .bind(body.memory_mib.max(0))
    .bind(&body.firewall_profile)
    .bind(body.firewall_enabled)
    .bind(body.bmc_vlan.trim())
    .bind(body.pxe_vlan.trim())
    .bind(body.bmc_username.trim())
    .bind(password_enc)
    .bind(mac)
    .bind(body.zone.trim())
    .execute(pool)
    .await?;

    let _ = crate::engine::zeus_firewall::metal::upsert_gitops_policy(
        pool,
        body.hostname.trim(),
        &body.firewall_profile,
    )
    .await;

    crate::db::query_as::<_, BaremetalServer>(
        "SELECT id, hostname, bmc_address, bmc_type, state, cpu_cores, memory_mib,
                firewall_profile, firewall_enabled, bmc_vlan, pxe_vlan, created_at,
                (bmc_password_enc <> '') AS bmc_credentials_configured, last_error,
                lifecycle, zone, mac_address
         FROM baremetal_servers WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

pub async fn link_host_firewall_profile(
    pool: &DbPool,
    baremetal_id: Uuid,
    host_id: Uuid,
) -> anyhow::Result<()> {
    let profile: String =
        crate::db::query_scalar("SELECT firewall_profile FROM baremetal_servers WHERE id = ?")
            .bind(baremetal_id)
            .fetch_one(pool)
            .await?;
    crate::db::query(
        "UPDATE hosts SET baremetal_origin_id = ?, notes = COALESCE(notes, '') || ? WHERE id = ?",
    )
    .bind(baremetal_id)
    .bind(format!(
        "\n[zeus] metal profile {profile} (policy stub until agent apply)"
    ))
    .bind(host_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub fn plan_capacity(query: &str) -> BaremetalCapacityPlan {
    let ql = query.to_lowercase();
    let engineers = ql
        .split_whitespace()
        .find_map(|w| w.parse::<i32>().ok())
        .unwrap_or(100)
        .clamp(1, 100_000);
    let servers_needed = ((engineers as f64) / 25.0).ceil() as i32;
    let cores_per = 64;
    let mem_gib_per = 512;
    BaremetalCapacityPlan {
        query: query.into(),
        servers_needed,
        total_cpu_cores: servers_needed.saturating_mul(cores_per),
        total_memory_gib: servers_needed.saturating_mul(mem_gib_per),
        summary: format!(
            "For ~{engineers} AI engineers: {servers_needed} bare-metal servers ({cores_per} cores, {mem_gib_per} GiB each)"
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct BmcPowerBody {
    pub action: String,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
pub struct BmcPowerResult {
    pub server_id: String,
    pub hostname: String,
    pub action: String,
    pub previous_state: String,
    pub new_state: String,
    pub dry_run: bool,
    pub summary: String,
}

pub async fn set_power(
    pool: &DbPool,
    id: Uuid,
    body: &BmcPowerBody,
) -> anyhow::Result<BmcPowerResult> {
    let action = PowerAction::parse(&body.action)?;
    let action_name = body.action.trim().to_ascii_lowercase();

    let row: BaremetalServer = crate::db::query_as(
        "SELECT id, hostname, bmc_address, bmc_type, state, cpu_cores, memory_mib,
                firewall_profile, firewall_enabled, bmc_vlan, pxe_vlan, created_at,
                (bmc_password_enc <> '') AS bmc_credentials_configured, last_error,
                lifecycle, zone, mac_address
         FROM baremetal_servers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("server not found"))?;

    let new_state = match action {
        PowerAction::On => "powered_on",
        PowerAction::Off => "powered_off",
        PowerAction::Cycle | PowerAction::Reset => "rebooting",
    };

    if body.dry_run {
        return Ok(BmcPowerResult {
            server_id: id.to_string(),
            hostname: row.hostname.clone(),
            action: action_name.clone(),
            previous_state: row.state.clone(),
            new_state: new_state.into(),
            dry_run: true,
            summary: format!(
                "Preview: BMC {} on {} ({}) — would transition {} → {}",
                action_name, row.hostname, row.bmc_address, row.state, new_state
            ),
        });
    }

    let kind = BmcKind::parse(&row.bmc_type)?;
    let (user, enc): (String, String) = crate::db::query_as(
        "SELECT bmc_username, bmc_password_enc FROM baremetal_servers WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    if !matches!(kind, BmcKind::Fake | BmcKind::Libvirt) && (user.is_empty() || enc.is_empty()) {
        anyhow::bail!(
            "BMC credentials are not set for {}; register it with bmc_username and bmc_password",
            row.hostname
        );
    }
    let password = if enc.is_empty() {
        String::new()
    } else {
        crate::engine::ai::crypto::load_api_key(&enc)?
    };

    let outcome = power(
        &BmcTarget {
            kind,
            address: &row.bmc_address,
            username: &user,
            password: &password,
        },
        action,
    )
    .await;

    match outcome {
        Ok(summary) => {
            crate::db::query(
                "UPDATE baremetal_servers SET state = ?, last_error = '' WHERE id = ?",
            )
            .bind(new_state)
            .bind(id)
            .execute(pool)
            .await?;
            Ok(BmcPowerResult {
                server_id: id.to_string(),
                hostname: row.hostname,
                action: action_name,
                previous_state: row.state,
                new_state: new_state.into(),
                dry_run: false,
                summary,
            })
        }
        Err(e) => {
            // State stays as it was: nothing changed on the machine.
            let msg = format!("{e:#}");
            let _ = crate::db::query("UPDATE baremetal_servers SET last_error = ? WHERE id = ?")
                .bind(&msg)
                .bind(id)
                .execute(pool)
                .await;
            Err(e)
        }
    }
}

/// A server's BMC details with the password decrypted, for one call.
pub struct BmcCreds {
    pub kind: BmcKind,
    pub address: String,
    pub username: String,
    pub password: String,
}

impl BmcCreds {
    pub fn target(&self) -> BmcTarget<'_> {
        BmcTarget {
            kind: self.kind,
            address: &self.address,
            username: &self.username,
            password: &self.password,
        }
    }
}

/// The BMC details of `id`, or `None` when there is nothing to call (a real BMC type
/// without credentials, or no address).
pub async fn load_bmc(pool: &DbPool, id: Uuid) -> anyhow::Result<Option<BmcCreds>> {
    let row: (String, String, String, String) = crate::db::query_as(
        "SELECT bmc_type, bmc_address, bmc_username, bmc_password_enc
         FROM baremetal_servers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("server not found"))?;
    let kind = BmcKind::parse(&row.0)?;
    let needs_creds = !matches!(kind, BmcKind::Fake | BmcKind::Libvirt);
    if needs_creds && (row.1.trim().is_empty() || row.2.is_empty() || row.3.is_empty()) {
        return Ok(None);
    }
    let password = if row.3.is_empty() {
        String::new()
    } else {
        crate::engine::ai::crypto::load_api_key(&row.3)?
    };
    Ok(Some(BmcCreds {
        kind,
        address: row.1,
        username: row.2,
        password,
    }))
}

fn power_column(p: PowerState) -> Option<&'static str> {
    match p {
        PowerState::On => Some("powered_on"),
        PowerState::Off => Some("powered_off"),
        PowerState::Unknown => None,
    }
}

/// Ask the BMC for the real power state and record it in `state`.
pub async fn refresh_power_state(pool: &DbPool, id: Uuid) -> anyhow::Result<PowerState> {
    let c = load_bmc(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("BMC credentials are not set for this server"))?;
    let st = super::baremetal_bmc::power_state(&c.target()).await?;
    if let Some(col) = power_column(st) {
        crate::db::query("UPDATE baremetal_servers SET state = ? WHERE id = ?")
            .bind(col)
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(st)
}

/// Set the next boot device through the BMC.
pub async fn set_boot_device(
    pool: &DbPool,
    id: Uuid,
    device: BootDevice,
    persistent: bool,
    uefi: Option<bool>,
) -> anyhow::Result<String> {
    let c = load_bmc(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("BMC credentials are not set for this server"))?;
    super::baremetal_bmc::set_boot_device(&c.target(), device, persistent, uefi).await
}

/// `set_boot_device` for the provisioning flows: a machine without usable BMC details (it
/// PXE-boots by its own boot order) is not an error. The outcome is returned for the log.
pub async fn set_boot_best_effort(pool: &DbPool, id: Uuid, device: BootDevice) -> String {
    match load_bmc(pool, id).await {
        Ok(Some(c)) => {
            match super::baremetal_bmc::set_boot_device(&c.target(), device, false, None).await {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("boot override {} for {id}: {e:#}", device.as_str());
                    let _ = crate::db::query(
                        "UPDATE baremetal_servers SET last_error = ? WHERE id = ?",
                    )
                    .bind(format!("boot override: {e}"))
                    .bind(id)
                    .execute(pool)
                    .await;
                    format!("boot override failed: {e}")
                }
            }
        }
        Ok(None) => "no BMC credentials: boot order left as is".into(),
        Err(e) => format!("boot override skipped: {e}"),
    }
}

/// Opt-in poll (`MACHINA_BMC_POLL_SECS`, default off) that keeps `state` equal to what the
/// BMCs report. Sequential, with each call bounded by the driver's own timeouts.
pub fn spawn_power_poll(state: crate::state::AppState) {
    let secs: u64 = std::env::var("MACHINA_BMC_POLL_SECS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    if secs == 0 {
        return;
    }
    tokio::spawn(async move {
        let mut interval = std::time::Duration::from_secs(secs);
        if interval.as_secs() < 30 {
            interval = std::time::Duration::from_secs(30);
        }
        let mut t = tokio::time::interval(interval);
        t.tick().await;
        loop {
            t.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            let ids: Vec<Uuid> = crate::db::query_scalar(
                "SELECT id FROM baremetal_servers WHERE bmc_address <> '' ORDER BY hostname",
            )
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();
            for id in ids {
                if let Err(e) = refresh_power_state(&state.pool, id).await {
                    tracing::debug!("BMC power poll {id}: {e:#}");
                }
            }
        }
    });
}

#[derive(Debug, Serialize)]
pub struct BaremetalProvisionPlan {
    pub server_id: String,
    pub hostname: String,
    pub steps: Vec<String>,
    pub summary: String,
}

pub async fn provision_preview(pool: &DbPool, id: Uuid) -> anyhow::Result<BaremetalProvisionPlan> {
    let row: BaremetalServer = crate::db::query_as(
        "SELECT id, hostname, bmc_address, bmc_type, state, cpu_cores, memory_mib,
                firewall_profile, firewall_enabled, bmc_vlan, pxe_vlan, created_at,
                (bmc_password_enc <> '') AS bmc_credentials_configured, last_error,
                lifecycle, zone, mac_address
         FROM baremetal_servers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("server not found"))?;

    let steps = vec![
        format!(
            "PXE boot {} via BMC {} (VLAN {})",
            row.hostname, row.bmc_address, row.pxe_vlan
        ),
        "Match hardware profile to image catalog (Ubuntu 24.04 / RHEL 9)".into(),
        format!(
            "Apply Zeus profile {} on provisioning network",
            row.firewall_profile
        ),
        "Register host in Machina fleet after first boot".into(),
    ];

    Ok(BaremetalProvisionPlan {
        server_id: id.to_string(),
        hostname: row.hostname.clone(),
        steps,
        summary: format!(
            "PXE provision preview for {} ({}) — boot service and deploy are not implemented yet",
            row.hostname, row.bmc_type
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(bmc_type: &str, user: &str, pass: &str) -> RegisterBaremetalBody {
        RegisterBaremetalBody {
            hostname: format!("metal-{bmc_type}-{}", Uuid::new_v4().simple()),
            bmc_address: "10.0.0.9".into(),
            bmc_type: bmc_type.into(),
            cpu_cores: 0,
            memory_mib: 0,
            firewall_profile: "BareMetalBmc".into(),
            firewall_enabled: true,
            bmc_vlan: String::new(),
            pxe_vlan: String::new(),
            bmc_username: user.into(),
            bmc_password: pass.into(),
            mac_address: String::new(),
            zone: String::new(),
        }
    }

    #[tokio::test]
    async fn fake_bmc_power_updates_state_and_hides_the_password() {
        let pool = crate::db::testing::pool().await;
        let s = register(&pool, &body("fake", "admin", "s3cret"))
            .await
            .unwrap();
        assert!(s.bmc_credentials_configured);
        assert!(!serde_json::to_string(&s).unwrap().contains("s3cret"));
        let r = set_power(
            &pool,
            s.id,
            &BmcPowerBody {
                action: "on".into(),
                dry_run: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(r.new_state, "powered_on");
        let after = list_servers(&pool).await.unwrap();
        assert_eq!(
            after.iter().find(|x| x.id == s.id).unwrap().state,
            "powered_on"
        );
    }

    #[tokio::test]
    async fn live_power_without_credentials_is_refused_and_state_kept() {
        let pool = crate::db::testing::pool().await;
        let s = register(&pool, &body("redfish", "", "")).await.unwrap();
        assert!(!s.bmc_credentials_configured);
        let err = set_power(
            &pool,
            s.id,
            &BmcPowerBody {
                action: "off".into(),
                dry_run: false,
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("credentials"));
        let now = list_servers(&pool).await.unwrap();
        assert_eq!(
            now.iter().find(|x| x.id == s.id).unwrap().state,
            "registered"
        );
    }

    #[tokio::test]
    async fn register_rejects_unknown_bmc_type_and_bad_address() {
        let pool = crate::db::testing::pool().await;
        assert!(register(&pool, &body("amt", "a", "b")).await.is_err());
        let mut b = body("ipmi", "a", "b");
        b.bmc_address = "-oProxyCommand=x".into();
        assert!(register(&pool, &b).await.is_err());
    }
}
