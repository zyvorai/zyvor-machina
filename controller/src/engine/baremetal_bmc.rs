// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Live BMC power control for bare-metal servers: Redfish, IPMI (`ipmitool`) and a
//! `fake` driver that touches no hardware (tests, labs, demos).
//!
//! The controller talks to the BMC directly, so power works while the host is dead.
//! Nothing here persists anything; `engine::baremetal::set_power` owns the database.

use std::time::Duration;

use serde_json::{json, Value};

/// Power action, as accepted by `POST /api/v1/baremetal/servers/{id}/power`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    On,
    Off,
    Cycle,
    Reset,
}

impl PowerAction {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "on" => Ok(Self::On),
            "off" => Ok(Self::Off),
            "cycle" => Ok(Self::Cycle),
            "reset" => Ok(Self::Reset),
            _ => anyhow::bail!("action must be on, off, cycle, or reset"),
        }
    }

    /// Redfish `ResetType` (DMTF DSP2046).
    pub fn redfish_reset_type(self) -> &'static str {
        match self {
            Self::On => "On",
            Self::Off => "ForceOff",
            Self::Cycle => "PowerCycle",
            Self::Reset => "ForceRestart",
        }
    }

    /// `ipmitool power <verb>`.
    pub fn ipmi_verb(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Cycle => "cycle",
            Self::Reset => "reset",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BmcKind {
    Redfish,
    Ipmi,
    Fake,
}

impl BmcKind {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "redfish" | "" => Ok(Self::Redfish),
            "ipmi" => Ok(Self::Ipmi),
            "fake" => Ok(Self::Fake),
            other => anyhow::bail!("unsupported bmc_type '{other}' (redfish, ipmi or fake)"),
        }
    }
}

pub struct BmcTarget<'a> {
    pub kind: BmcKind,
    pub address: &'a str,
    pub username: &'a str,
    pub password: &'a str,
}

/// BMCs ship self-signed certificates, so verification is off unless asked for.
/// The BMC network is a management VLAN; set `MACHINA_BMC_VERIFY_TLS=1` when the
/// BMCs carry certificates from a CA the controller trusts.
fn verify_tls() -> bool {
    matches!(
        std::env::var("MACHINA_BMC_VERIFY_TLS").ok().as_deref(),
        Some("1" | "true" | "yes")
    )
}

/// An address (or `host:port`) that is safe to put on an `ipmitool` argv and in a URL.
pub fn validate_address(addr: &str) -> anyhow::Result<()> {
    let a = addr.trim();
    if a.is_empty() {
        anyhow::bail!("BMC address is not set");
    }
    if a.starts_with('-') {
        anyhow::bail!("invalid BMC address");
    }
    let body = a
        .strip_prefix("https://")
        .or_else(|| a.strip_prefix("http://"))
        .unwrap_or(a)
        .trim_end_matches('/');
    if body.is_empty()
        || !body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_' | '[' | ']'))
    {
        anyhow::bail!("invalid BMC address '{a}'");
    }
    Ok(())
}

fn validate_username(user: &str) -> anyhow::Result<()> {
    if user.is_empty() || user.starts_with('-') || user.chars().any(|c| c.is_control()) {
        anyhow::bail!("invalid BMC username");
    }
    Ok(())
}

/// `https://addr` unless the operator gave a scheme.
pub fn base_url(addr: &str) -> String {
    let a = addr.trim().trim_end_matches('/');
    if a.starts_with("https://") || a.starts_with("http://") {
        a.to_string()
    } else {
        format!("https://{a}")
    }
}

/// First member of a Redfish `Systems` collection document.
pub fn first_system_uri(collection: &Value) -> Option<String> {
    collection
        .get("Members")?
        .as_array()?
        .iter()
        .find_map(|m| m.get("@odata.id").and_then(Value::as_str))
        .map(str::to_string)
}

/// Reset action target from a `ComputerSystem` document, with the standard path as fallback.
pub fn reset_target(system: &Value, system_uri: &str) -> String {
    system
        .pointer("/Actions/#ComputerSystem.Reset/target")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "{}/Actions/ComputerSystem.Reset",
                system_uri.trim_end_matches('/')
            )
        })
}

/// Apply `action` through the BMC; returns a one-line summary of what happened.
pub async fn power(target: &BmcTarget<'_>, action: PowerAction) -> anyhow::Result<String> {
    match target.kind {
        BmcKind::Fake => Ok(format!(
            "fake BMC: power {} (no hardware)",
            action.ipmi_verb()
        )),
        BmcKind::Ipmi => ipmi_power(target, action).await,
        BmcKind::Redfish => redfish_power(target, action).await,
    }
}

async fn ipmi_power(target: &BmcTarget<'_>, action: PowerAction) -> anyhow::Result<String> {
    validate_address(target.address)?;
    validate_username(target.username)?;
    // `-H` takes a bare host; drop a scheme the operator may have typed.
    let host = target
        .address
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    // Password via IPMI_PASSWORD (`-E`), never on argv where ps/proc would show it.
    let out = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new("ipmitool")
            .args([
                "-I",
                "lanplus",
                "-H",
                host,
                "-U",
                target.username,
                "-E",
                "power",
                action.ipmi_verb(),
            ])
            .env("IPMI_PASSWORD", target.password)
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("ipmitool timed out after 30s"))?
    .map_err(|e| {
        anyhow::anyhow!("ipmitool exec failed (is it installed on the controller host?): {e}")
    })?;
    if out.status.success() {
        Ok(format!("IPMI power {} on {host}", action.ipmi_verb()))
    } else {
        anyhow::bail!(
            "ipmitool power {} failed: {}",
            action.ipmi_verb(),
            String::from_utf8_lossy(&out.stderr).trim()
        )
    }
}

async fn redfish_power(target: &BmcTarget<'_>, action: PowerAction) -> anyhow::Result<String> {
    validate_address(target.address)?;
    validate_username(target.username)?;
    let base = base_url(target.address);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .danger_accept_invalid_certs(!verify_tls())
        .build()?;
    let get = |path: String| {
        let client = client.clone();
        let (user, pass) = (target.username.to_string(), target.password.to_string());
        async move {
            let resp = client
                .get(&path)
                .basic_auth(user, Some(pass))
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("Redfish GET {path}: {e}"))?;
            if !resp.status().is_success() {
                anyhow::bail!("Redfish GET {path}: HTTP {}", resp.status());
            }
            resp.json::<Value>()
                .await
                .map_err(|e| anyhow::anyhow!("Redfish GET {path}: bad JSON: {e}"))
        }
    };

    let collection = get(format!("{base}/redfish/v1/Systems")).await?;
    let system_uri = first_system_uri(&collection)
        .ok_or_else(|| anyhow::anyhow!("Redfish: no ComputerSystem in /redfish/v1/Systems"))?;
    let system = get(format!("{base}{system_uri}")).await?;
    let reset = reset_target(&system, &system_uri);

    let url = format!("{base}{reset}");
    let resp = client
        .post(&url)
        .basic_auth(target.username, Some(target.password))
        .json(&json!({ "ResetType": action.redfish_reset_type() }))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Redfish POST {url}: {e}"))?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "Redfish reset {} rejected: HTTP {}",
            action.redfish_reset_type(),
            resp.status()
        );
    }
    Ok(format!(
        "Redfish {} on {}",
        action.redfish_reset_type(),
        target.address.trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_map_to_redfish_and_ipmi() {
        assert_eq!(PowerAction::parse("ON").unwrap().redfish_reset_type(), "On");
        assert_eq!(
            PowerAction::parse("off").unwrap().redfish_reset_type(),
            "ForceOff"
        );
        assert_eq!(
            PowerAction::parse("cycle").unwrap().redfish_reset_type(),
            "PowerCycle"
        );
        assert_eq!(PowerAction::parse("reset").unwrap().ipmi_verb(), "reset");
        assert!(PowerAction::parse("halt").is_err());
    }

    #[test]
    fn bmc_kind_defaults_to_redfish_and_rejects_unknown() {
        assert_eq!(BmcKind::parse("").unwrap(), BmcKind::Redfish);
        assert_eq!(BmcKind::parse("IPMI").unwrap(), BmcKind::Ipmi);
        assert!(BmcKind::parse("amt").is_err());
    }

    #[test]
    fn address_validation_blocks_argv_and_url_tricks() {
        assert!(validate_address("10.0.0.5").is_ok());
        assert!(validate_address("bmc1.rack2.example:8443").is_ok());
        assert!(validate_address("https://[fd00::5]").is_ok());
        assert!(validate_address("").is_err());
        assert!(validate_address("-oProxyCommand=x").is_err());
        assert!(validate_address("10.0.0.5/../admin").is_err());
        assert!(validate_address("host name").is_err());
        assert!(validate_username("ADMIN").is_ok());
        assert!(validate_username("-U").is_err());
    }

    #[test]
    fn base_url_adds_https_only_without_a_scheme() {
        assert_eq!(base_url("10.0.0.5"), "https://10.0.0.5");
        assert_eq!(base_url("http://10.0.0.5/"), "http://10.0.0.5");
    }

    #[test]
    fn redfish_documents_resolve() {
        let coll = json!({"Members": [{"@odata.id": "/redfish/v1/Systems/System.Embedded.1"}]});
        let uri = first_system_uri(&coll).unwrap();
        assert_eq!(uri, "/redfish/v1/Systems/System.Embedded.1");
        let with_action = json!({"Actions": {"#ComputerSystem.Reset": {"target": "/x/Reset"}}});
        assert_eq!(reset_target(&with_action, &uri), "/x/Reset");
        assert_eq!(
            reset_target(&json!({}), &uri),
            "/redfish/v1/Systems/System.Embedded.1/Actions/ComputerSystem.Reset"
        );
        assert!(first_system_uri(&json!({"Members": []})).is_none());
    }

    #[tokio::test]
    async fn fake_driver_needs_no_hardware() {
        let t = BmcTarget {
            kind: BmcKind::Fake,
            address: "",
            username: "",
            password: "",
        };
        let s = power(&t, PowerAction::Cycle).await.unwrap();
        assert!(s.contains("fake BMC"));
    }
}
