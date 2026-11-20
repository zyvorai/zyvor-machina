// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Enable Remote Desktop on a **powered-off** Windows guest by editing its
//! registry hive offline via **GuestKit only** (`plan apply --skip-backup`,
//! `--features registry-write` / libhivex).
//!
//! Machina does **not** call `virt-win-reg` or other libguestfs-tools CLIs.
//! Disk mount/hive transfer stays inside GuestKit.
//!
//! The plan mirrors hyper2kvm firstboot RDP enablement: Terminal Server allow,
//! NLA, TermService/UmRdpService Automatic, stock inbound firewall rules
//! Active=TRUE.
//!
//! This path needs nothing inside the guest: no QEMU guest agent, no in-guest
//! agent, no console trip. Remote Desktop ships disabled on Windows, so a
//! freshly-installed guest is unreachable over RDP until somebody turns it on.

use std::path::Path;
use std::process::Command;

use crate::LibvirtError;

/// `fDenyTSConnections = 0` is what the Remote Desktop toggle actually writes.
///
/// Note the control set is spelled out rather than using `CurrentControlSet`.
/// That name is a runtime symlink Windows builds at boot from `Select\Current`
/// and does **not** exist in an offline hive — writing via `CurrentControlSet`
/// creates a literal key Windows never reads.
const TS_KEY: &str = r"HKLM\SYSTEM\ControlSet001\Control\Terminal Server";
const TS_VALUE: &str = "fDenyTSConnections";

/// Network Level Authentication — left on, but the value must exist for the
/// service to start cleanly on some images.
const NLA_KEY: &str = r"HKLM\SYSTEM\ControlSet001\Control\Terminal Server\WinStations\RDP-Tcp";

/// Windows Firewall rule store. Enabling the Terminal Server service is not
/// enough on its own: the inbound RDP rule ships `Active=FALSE`, so the firewall
/// silently drops 3389 and the connection times out rather than refusing.
const FW_RULES_KEY: &str =
    r"HKLM\SYSTEM\ControlSet001\Services\SharedAccess\Parameters\FirewallPolicy\FirewallRules";

/// TermService must be Automatic (Start=2) or nothing listens on 3389 after boot.
const TERM_SERVICE_KEY: &str = r"HKLM\SYSTEM\ControlSet001\Services\TermService";
/// Session shadow / multi-transport helper used by modern RDP stacks.
const UM_RDP_SERVICE_KEY: &str = r"HKLM\SYSTEM\ControlSet001\Services\UmRdpService";
/// SERVICE_AUTO_START
const SERVICE_START_AUTO: u32 = 2;

/// The built-in inbound RDP rules present on a stock Windows image.
const FW_RDP_RULES: [&str; 2] = [
    "RemoteDesktop-UserMode-In-TCP",
    "RemoteDesktop-UserMode-In-UDP",
];

/// Stock Win10/11 firewall rule blobs with Active=TRUE (v2.29 schema).
/// Offline we stage allow rules into the hive (same outcome as firstboot
/// `netsh … group="remote desktop"`).
fn stock_firewall_rule(name: &str) -> &'static str {
    match name {
        "RemoteDesktop-UserMode-In-TCP" => {
            "v2.29|Action=Allow|Active=TRUE|Dir=In|Protocol=6|LPort=3389|\
App=%SystemRoot%\\system32\\svchost.exe|Svc=termservice|\
Name=@FirewallAPI.dll,-28753|Desc=@FirewallAPI.dll,-28756|\
EmbedCtxt=@FirewallAPI.dll,-28752|"
        }
        "RemoteDesktop-UserMode-In-UDP" => {
            "v2.29|Action=Allow|Active=TRUE|Dir=In|Protocol=17|LPort=3389|\
App=%SystemRoot%\\system32\\svchost.exe|Svc=termservice|\
Name=@FirewallAPI.dll,-28752|Desc=@FirewallAPI.dll,-28756|\
EmbedCtxt=@FirewallAPI.dll,-28752|"
        }
        _ => "",
    }
}

/// Flip a firewall rule string from inactive to active (unit-test helper).
pub fn activate_firewall_rule(rule: &str) -> Option<String> {
    if rule.contains("Active=TRUE") {
        return None;
    }
    if !rule.contains("Active=FALSE") {
        return None;
    }
    Some(rule.replace("Active=FALSE", "Active=TRUE"))
}

/// Stock Active=TRUE firewall edits for the GuestKit plan (no host CLI probes).
pub fn stock_firewall_edits() -> Vec<(String, String)> {
    FW_RDP_RULES
        .iter()
        .filter_map(|rule| {
            let stock = stock_firewall_rule(rule);
            if stock.is_empty() {
                None
            } else {
                Some(((*rule).to_string(), stock.to_string()))
            }
        })
        .collect()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RdpEnableOutcome {
    pub disk_path: String,
    /// Registry values written / intended by the plan.
    pub applied: Vec<String>,
    /// True when no firewall rule edits were staged.
    pub firewall_manual: bool,
    pub notes: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
fn registry_edit_op(
    id: &str,
    key: &str,
    value: &str,
    current: serde_json::Value,
    new_data: serde_json::Value,
    data_type: &str,
    priority: &str,
    description: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "type": "registry_edit",
        "key": key,
        "value": value,
        "current_data": current,
        "new_data": new_data,
        "data_type": data_type,
        "priority": priority,
        "description": description,
        "risk": "low",
        "reversible": true,
    })
}

/// Build the GuestKit fix-plan JSON for the full Remote Desktop stack.
///
/// Feats aligned with hyper2kvm firstboot RDP enablement:
/// `fDenyTSConnections`, NLA, TermService/UmRdpService Automatic, RDP port,
/// inbound TCP+UDP firewall Active=TRUE.
pub fn build_rdp_enable_plan(
    disk_path: &str,
    generated_rfc3339: &str,
    firewall_edits: &[(String, String)],
) -> serde_json::Value {
    let mut operations = vec![
        registry_edit_op(
            "enable-rdp",
            TS_KEY,
            TS_VALUE,
            serde_json::json!(1),
            serde_json::json!(0),
            "dword",
            "high",
            "Allow Remote Desktop connections",
        ),
        registry_edit_op(
            "rdp-nla",
            NLA_KEY,
            "UserAuthentication",
            serde_json::json!(1),
            serde_json::json!(1),
            "dword",
            "low",
            "Keep Network Level Authentication enabled",
        ),
        registry_edit_op(
            "rdp-port",
            NLA_KEY,
            "PortNumber",
            serde_json::json!(3389),
            serde_json::json!(3389),
            "dword",
            "low",
            "Ensure RDP listens on TCP 3389",
        ),
        registry_edit_op(
            "termservice-auto",
            TERM_SERVICE_KEY,
            "Start",
            serde_json::json!(3),
            serde_json::json!(SERVICE_START_AUTO),
            "dword",
            "high",
            "Set TermService startup type to Automatic",
        ),
        registry_edit_op(
            "umrdpservice-auto",
            UM_RDP_SERVICE_KEY,
            "Start",
            serde_json::json!(3),
            serde_json::json!(SERVICE_START_AUTO),
            "dword",
            "high",
            "Set UmRdpService startup type to Automatic",
        ),
    ];

    for (i, (rule_name, new_value)) in firewall_edits.iter().enumerate() {
        operations.push(registry_edit_op(
            &format!("fw-{i}"),
            FW_RULES_KEY,
            rule_name,
            serde_json::json!(""),
            serde_json::json!(new_value),
            "sz",
            "high",
            &format!("Enable firewall rule {rule_name}"),
        ));
    }

    serde_json::json!({
        "version": "1",
        "vm": disk_path,
        "generated": generated_rfc3339,
        "profile": "machina-enable-rdp",
        "overall_risk": "low",
        "estimated_duration": "seconds",
        "metadata": {
            "author": "machina",
            "review_required": false,
            "reversible": true,
            "description": "Offline Windows RDP enablement via GuestKit (Terminal Server + NLA + TermService/UmRdpService + firewall)",
            "tags": ["windows", "rdp", "firewall", "offline", "guestkit"],
        },
        "operations": operations,
        "post_apply": [],
    })
}

/// Pull the path out of guestkit's "Backup created: /path" line.
fn parse_backup_path(out: &str) -> Option<String> {
    out.lines()
        .find_map(|l| l.trim().strip_prefix("Backup created:"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Pull `N` out of guestkit's "Operations applied: N" summary line.
fn parse_applied_count(out: &str) -> Option<u32> {
    out.lines().find_map(|l| {
        l.trim()
            .strip_prefix("Operations applied:")
            .and_then(|rest| rest.trim().parse().ok())
    })
}

/// Path to the guestkit binary, honouring the configured override.
fn guestkit_binary(configured: &str) -> String {
    let t = configured.trim();
    if t.is_empty() {
        "guestkit".to_string()
    } else {
        t.to_string()
    }
}

/// GuestKit `plan apply --skip-backup` — hivex hive writes without full qcow2 copy.
fn apply_rdp_via_guestkit(
    disk_path: &str,
    guestkit_bin: &str,
    firewall_edits: &[(String, String)],
) -> Result<Option<String>, LibvirtError> {
    let generated = chrono::Utc::now().to_rfc3339();
    let plan = build_rdp_enable_plan(disk_path, &generated, firewall_edits);
    let plan_file =
        std::env::temp_dir().join(format!("machina-rdp-plan-{}.json", std::process::id()));
    std::fs::write(
        &plan_file,
        serde_json::to_vec_pretty(&plan).unwrap_or_default(),
    )
    .map_err(|e| LibvirtError::Operation(format!("cannot write fix plan: {e}")))?;

    let bin = guestkit_binary(guestkit_bin);
    let out = Command::new(&bin)
        .arg("plan")
        .arg("apply")
        .arg(&plan_file)
        .arg("--vm")
        .arg(disk_path)
        .arg("--yes")
        .arg("--skip-backup")
        .output();
    let _ = std::fs::remove_file(&plan_file);

    let out = out.map_err(|e| {
        LibvirtError::Operation(format!(
            "cannot run {bin}: {e} — install guestkit on the hypervisor \
             (rebuild with --features registry-write)"
        ))
    })?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    if !out.status.success()
        && (stderr.contains("unexpected argument '--skip-backup'")
            || stderr.contains("Unrecognized option")
            || combined_help_mentions_unknown_skip(&stdout, &stderr))
    {
        return Err(LibvirtError::Operation(
            "guestkit on this host is too old for `plan apply --skip-backup` \
             (needs GuestKit ≥ 0.3.16 with registry-write). Upgrade guestkit."
                .into(),
        ));
    }

    if stdout.contains("--features registry-write") || stderr.contains("--features registry-write")
    {
        return Err(LibvirtError::Operation(
            "guestkit on this host was built without offline registry writes \
             (rebuild with --features registry-write)"
                .into(),
        ));
    }
    if !out.status.success() {
        let msg = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(LibvirtError::Operation(format!(
            "guestkit plan apply failed: {msg}"
        )));
    }

    let guestkit_backup = parse_backup_path(&stdout).or_else(|| parse_backup_path(&stderr));
    let applied_count = parse_applied_count(&stdout).or_else(|| parse_applied_count(&stderr));
    match applied_count {
        Some(n) if n > 0 => Ok(guestkit_backup),
        Some(_) => Err(LibvirtError::Operation(format!(
            "guestkit applied 0 operations — the registry was not written. \
             If the guest filesystem mounted read-only, boot the VM and shut it \
             down cleanly from inside Windows (or run ntfsfix -d on the offline \
             volume), then retry. guestkit said: {}",
            stdout
                .lines()
                .find(|l| l.contains("failed:") || l.contains("Read-only"))
                .unwrap_or("(no detail)")
                .trim()
        ))),
        None => Err(LibvirtError::Operation(
            "guestkit exited 0 but reported no applied operations, so the registry \
             was not written. Upgrade guestkit or enable Remote Desktop inside the guest."
                .into(),
        )),
    }
}

fn combined_help_mentions_unknown_skip(stdout: &str, stderr: &str) -> bool {
    let blob = format!("{stdout}\n{stderr}");
    blob.contains("skip-backup")
        && (blob.contains("error:") || blob.contains("Unknown") || blob.contains("unknown"))
}

fn rdp_outcome(disk_path: &str, firewall_edits: &[(String, String)]) -> RdpEnableOutcome {
    let mut notes = vec![
        "Registry written via guestkit plan apply --skip-backup.".to_string(),
        "Start the VM — Remote Desktop is now enabled in the registry.".to_string(),
        "Windows Home editions cannot host RDP regardless of this setting.".to_string(),
    ];
    if firewall_edits.is_empty() {
        notes.insert(
            0,
            "No inbound RDP firewall rule could be staged — open 'Remote Desktop' \
             in Windows Firewall inside the guest, or the port will still be dropped."
                .to_string(),
        );
    } else {
        notes.insert(
            0,
            format!(
                "Staged {} inbound RDP firewall rule(s): {}.",
                firewall_edits.len(),
                firewall_edits
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }

    let mut applied = vec![
        format!("{TS_KEY}\\{TS_VALUE} = 0"),
        format!("{NLA_KEY}\\UserAuthentication = 1"),
        format!("{NLA_KEY}\\PortNumber = 3389"),
        format!("{TERM_SERVICE_KEY}\\Start = {SERVICE_START_AUTO} (Automatic)"),
        format!("{UM_RDP_SERVICE_KEY}\\Start = {SERVICE_START_AUTO} (Automatic)"),
    ];
    for (name, _) in firewall_edits {
        applied.push(format!("{FW_RULES_KEY}\\{name} Active=TRUE"));
    }

    RdpEnableOutcome {
        disk_path: disk_path.to_string(),
        applied,
        firewall_manual: firewall_edits.is_empty(),
        notes,
    }
}

/// Apply RDP registry edits to `disk_path` via GuestKit only.
/// The VM **must be powered off** — mutating a hive under a running guest risks
/// corrupting it.
pub fn enable_rdp_offline(
    disk_path: &str,
    guestkit_bin: &str,
) -> Result<RdpEnableOutcome, LibvirtError> {
    let disk = Path::new(disk_path);
    if !disk.is_absolute() {
        return Err(LibvirtError::Invalid(
            "disk path must be absolute".to_string(),
        ));
    }
    if !disk.is_file() {
        return Err(LibvirtError::NotFound(format!(
            "disk image not found: {disk_path}"
        )));
    }

    let firewall_edits = stock_firewall_edits();
    let guestkit_backup = apply_rdp_via_guestkit(disk_path, guestkit_bin, &firewall_edits)?;
    // Prefer not to leave accidental full-image backups if an older guestkit
    // ignored --skip-backup (should not happen on ≥0.3.16).
    if let Some(ref b) = guestkit_backup {
        let _ = std::fs::remove_file(b);
    }

    Ok(rdp_outcome(disk_path, &firewall_edits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_matches_guestkit_fixplan_shape() {
        let fw = stock_firewall_edits();
        let p = build_rdp_enable_plan(
            "/var/lib/libvirt/images/win10.qcow2",
            "2026-07-19T00:00:00Z",
            &fw,
        );
        assert_eq!(p["version"], "1");
        assert_eq!(p["profile"], "machina-enable-rdp");
        assert!(p["post_apply"].is_array());
        assert!(p["operations"].as_array().unwrap().len() >= 5);
        let op = &p["operations"][0];
        assert_eq!(op["type"], "registry_edit");
        assert_eq!(op["value"], "fDenyTSConnections");
        assert_eq!(op["new_data"], 0);
        let ids: Vec<&str> = p["operations"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|o| o["id"].as_str())
            .collect();
        assert!(ids.contains(&"termservice-auto"));
        assert!(ids.contains(&"umrdpservice-auto"));
        assert!(ids.iter().any(|id| id.starts_with("fw-")));
    }

    #[test]
    fn enable_rdp_offline_rejects_bad_paths() {
        assert!(enable_rdp_offline("relative/win.qcow2", "guestkit").is_err());
        assert!(enable_rdp_offline("/nonexistent/win-does-not-exist.qcow2", "guestkit").is_err());
    }

    #[test]
    fn activate_firewall_rule_flips_active() {
        let rule = "v2.29|Action=Allow|Active=FALSE|Dir=In|Protocol=6|LPort=3389|";
        let got = activate_firewall_rule(rule).expect("flipped");
        assert!(got.contains("Active=TRUE"));
        assert!(!got.contains("Active=FALSE"));
        assert!(activate_firewall_rule(&got).is_none());
    }

    #[test]
    fn stock_firewall_rules_are_active() {
        let edits = stock_firewall_edits();
        assert_eq!(edits.len(), 2);
        for (name, rule) in &edits {
            assert!(rule.contains("Active=TRUE"), "{name}");
            assert!(rule.contains("LPort=3389"), "{name}");
        }
    }

    #[test]
    fn parse_applied_count_and_backup() {
        let preview_only = "\n📋 Fix Plan Preview\nVM: /x.qcow2\n[enable-rdp] Allow Remote Desktop connections\n  1 → 0\nBackup: Will create automatic backup\nRollback: Available for all operations\n";
        assert_eq!(parse_applied_count(preview_only), None);

        let out = "Backup created: /var/lib/libvirt/images/win10.backup_20260720_140610.qcow2\n✓ Plan applied successfully\n  Operations applied: 1\n";
        assert_eq!(
            parse_backup_path(out).as_deref(),
            Some("/var/lib/libvirt/images/win10.backup_20260720_140610.qcow2")
        );
        assert_eq!(parse_backup_path("Operations applied: 1\n"), None);
        assert_eq!(
            parse_applied_count(
                "✓ Plan applied successfully\n  Operations applied: 2\n  Operations skipped: 0\n"
            ),
            Some(2)
        );
        assert_eq!(
            parse_applied_count("  Operations applied: 0\n  Operations skipped: 1\n"),
            Some(0)
        );
    }

    #[test]
    fn guestkit_binary_defaults() {
        assert_eq!(guestkit_binary(""), "guestkit");
        assert_eq!(guestkit_binary("  "), "guestkit");
        assert_eq!(
            guestkit_binary("/usr/local/bin/guestkit"),
            "/usr/local/bin/guestkit"
        );
    }
}
