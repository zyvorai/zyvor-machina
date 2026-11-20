// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Default GuestKit in-guest agent (QGA-compatible) for new VMs.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::LibvirtConfig;
use crate::LibvirtError;

use super::subprocess::VmCreateLogSink;

pub const DEFAULT_CLOUD_INIT_USER: &str = "machina";

const GUESTKIT_UNIT: &str = r#"[Unit]
Description=GuestKit Agent (QGA-compatible virtio channel)
After=network.target
ConditionPathExists=/dev/virtio-ports/org.qemu.guest_agent.0
# Tolerate a burst of early-boot failures (e.g. the binary install from the seed racing
# service start) — 30 tries over 5 min — but still give up eventually so a genuinely
# absent binary doesn't restart-loop and spam the journal forever.
StartLimitIntervalSec=300
StartLimitBurst=30

[Service]
ExecStart=/usr/local/bin/guestkit agent --channel virtio
Restart=on-failure
RestartSec=5
User=root
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
"#;

/// Effective config when callers only have optional overrides.
pub fn effective_guest_agent_cfg(cfg: Option<&LibvirtConfig>) -> (bool, PathBuf) {
    cfg.map(|c| {
        (
            c.guest_agent_by_default,
            PathBuf::from(&c.guestkit_agent_binary),
        )
    })
    .unwrap_or((true, PathBuf::from("/usr/local/bin/guestkit")))
}

pub fn guest_agent_enabled(cfg: Option<&LibvirtConfig>) -> bool {
    effective_guest_agent_cfg(cfg).0
}

pub fn resolve_guestkit_binary(cfg: Option<&LibvirtConfig>) -> PathBuf {
    let (_, mut path) = effective_guest_agent_cfg(cfg);
    if !path.is_absolute() {
        path = PathBuf::from("/usr/local/bin/guestkit");
    }
    path
}

pub fn vm_wants_graphical_desktop(name: &str) -> bool {
    name.to_ascii_lowercase().contains("desktop")
}

/// Ensure GNOME/GDM autologin on first boot (cloud images default to serial/tty).
pub fn append_desktop_graphical_cloud_config(user_data: &mut String, login_user: &str) {
    if !user_data.ends_with('\n') {
        user_data.push('\n');
    }
    let user = if login_user.trim().is_empty() {
        "ubuntu"
    } else {
        login_user.trim()
    };
    let cmds = [
        "  - mkdir -p /etc/gdm3".to_string(),
        format!(
            "  - printf '%s\\n' '[daemon]' 'AutomaticLogin={user}' 'AutomaticLoginEnable=true' > /etc/gdm3/custom.conf"
        ),
        "  - [ systemctl, set-default, graphical.target ]".into(),
        "  - bash -lc 'systemctl enable gdm 2>/dev/null || systemctl enable gdm3 2>/dev/null || true'".into(),
        "  - bash -lc 'systemctl restart gdm 2>/dev/null || systemctl restart gdm3 2>/dev/null || true'".into(),
    ];
    if user_data.contains("runcmd:\n") {
        for c in cmds {
            user_data.push_str(&c);
            user_data.push('\n');
        }
    } else {
        user_data.push_str("runcmd:\n");
        for c in cmds {
            user_data.push_str(&c);
            user_data.push('\n');
        }
    }
}

/// Append cloud-config stanzas that install guestkit-agent from the NoCloud seed ISO.
pub fn append_guestkit_cloud_config(user_data: &mut String, binary_on_seed: bool) {
    if !user_data.ends_with('\n') {
        user_data.push('\n');
    }
    user_data.push_str("package_update: false\n");
    user_data.push_str("write_files:\n");
    user_data.push_str("  - path: /etc/systemd/system/guestkit-agent.service\n");
    user_data.push_str("    permissions: '0644'\n");
    user_data.push_str("    content: |\n");
    for line in GUESTKIT_UNIT.lines() {
        user_data.push_str("      ");
        user_data.push_str(line);
        user_data.push('\n');
    }
    if binary_on_seed {
        user_data.push_str("runcmd:\n");
        // Explicitly mount the NoCloud seed by its 'cidata' label and install the
        // guestkit binary from it. cloud-init does NOT leave the seed mounted at a
        // fixed path, so the fallback checks below (/mnt/cidata, /mnt/cdrom, …) miss
        // it — the unit then gets enabled but its ExecStart binary is absent, so the
        // agent never starts. Mounting by label is reliable: the seed ISO is labeled
        // 'cidata'. Do this BEFORE `systemctl enable --now` so the binary exists.
        user_data.push_str("  - mkdir -p /run/guestkit-seed\n");
        user_data.push_str(
            "  - mount -L cidata /run/guestkit-seed 2>/dev/null || mount /dev/sr0 /run/guestkit-seed 2>/dev/null || mount /dev/cdrom /run/guestkit-seed 2>/dev/null || true\n",
        );
        user_data.push_str(
            "  - test -f /run/guestkit-seed/guestkit && install -m755 /run/guestkit-seed/guestkit /usr/local/bin/guestkit || true\n",
        );
        user_data.push_str("  - umount /run/guestkit-seed 2>/dev/null || true\n");
        // Fallbacks for guests where the seed is already mounted somewhere findable.
        user_data.push_str(
            "  - test -x /usr/local/bin/guestkit || (test -f /mnt/cdrom/guestkit && install -m755 /mnt/cdrom/guestkit /usr/local/bin/guestkit) || true\n",
        );
        user_data.push_str(
            "  - test -x /usr/local/bin/guestkit || (test -f /mnt/cidata/guestkit && install -m755 /mnt/cidata/guestkit /usr/local/bin/guestkit) || true\n",
        );
        user_data.push_str(
            "  - test -x /usr/local/bin/guestkit || for m in /run/media/*/guestkit /media/*/guestkit; do [ -f \"$m\" ] && install -m755 \"$m\" /usr/local/bin/guestkit && break; done\n",
        );
    }
    user_data.push_str("  - systemctl daemon-reload\n");
    user_data.push_str("  - systemctl enable --now guestkit-agent\n");
}

/// Copy guestkit binary + optional marker into a seed ISO build directory.
pub fn stage_guestkit_seed_files(
    work_dir: &Path,
    cfg: Option<&LibvirtConfig>,
    log: Option<&VmCreateLogSink>,
) -> Result<(), LibvirtError> {
    if !guest_agent_enabled(cfg) {
        return Ok(());
    }
    let binary = resolve_guestkit_binary(cfg);
    if !binary.is_file() {
        super::subprocess::log_line(
            log,
            "machina",
            &format!(
                "guestkit-agent: binary not found at {} — install guestkit on hypervisor",
                binary.display()
            ),
        );
        return Ok(());
    }
    let dest = work_dir.join("guestkit");
    fs::copy(&binary, &dest).map_err(|e| {
        LibvirtError::Operation(format!(
            "copy guestkit agent to seed dir {}: {e}",
            dest.display()
        ))
    })?;
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o755)).map_err(|e| {
        LibvirtError::Operation(format!("chmod guestkit seed {}: {e}", dest.display()))
    })?;
    super::subprocess::log_line(
        log,
        "machina",
        &format!(
            "guestkit-agent staged for cloud-init ({})",
            binary.display()
        ),
    );
    Ok(())
}

/// Inject guestkit agent into a qcow2/raw disk (VM must be off).
pub fn inject_guestkit_into_disk(
    disk_path: &str,
    cfg: Option<&LibvirtConfig>,
    log: Option<&VmCreateLogSink>,
) -> Result<(), LibvirtError> {
    if !guest_agent_enabled(cfg) {
        return Ok(());
    }
    let disk = Path::new(disk_path);
    if !disk.is_file() {
        return Ok(());
    }
    let binary = resolve_guestkit_binary(cfg);
    if !binary.is_file() {
        super::subprocess::log_line(
            log,
            "machina",
            &format!("guestkit inject skipped: {} not found", binary.display()),
        );
        return Ok(());
    }
    let summary = format!(
        "$ guestkit repair {} --inject-agent --agent-binary {}",
        disk.display(),
        binary.display()
    );
    let mut cmd = Command::new("guestkit");
    cmd.args([
        "repair",
        disk.to_string_lossy().as_ref(),
        "--inject-agent",
        "--agent-binary",
        binary.to_string_lossy().as_ref(),
    ]);
    let out = super::subprocess::run_command_streaming(cmd, &summary, "guestkit", log)?;
    if !out.status.success() {
        super::subprocess::log_line(
            log,
            "machina",
            "guestkit inject-agent failed (non-fatal; use cloud-init seed)",
        );
    } else {
        super::subprocess::log_line(log, "machina", "guestkit-agent injected into disk image");
    }
    Ok(())
}

/// Result of an offline agent injection into an existing VM's disk.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentInjectReport {
    pub vm: String,
    pub disk: String,
    pub binary: String,
    pub dry_run: bool,
    /// Combined stdout/stderr from `guestkit agent-inject` (trimmed).
    pub output: String,
}

/// Install the GuestKit agent into an existing VM's primary disk with `guestkit agent-inject`.
///
/// The VM must be powered off: writing into a disk a running guest has open would corrupt it, so a
/// running domain is refused with a clear message instead of being stopped here. The caller (the web
/// UI) shuts the VM down first, calls this, then starts it again.
pub fn inject_agent_offline(
    conn: &virt::connect::Connect,
    name: &str,
    cfg: Option<&LibvirtConfig>,
    dry_run: bool,
) -> Result<AgentInjectReport, LibvirtError> {
    let domain = super::domain::lookup_domain(conn, name)?;
    let active = domain.is_active().map_err(|e| {
        LibvirtError::Operation(format!("Failed to read state of VM '{name}': {e}"))
    })?;
    if active {
        return Err(LibvirtError::Invalid(format!(
            "VM '{name}' is running — shut it down before injecting the guest agent"
        )));
    }

    let xml = domain
        .get_xml_desc(0)
        .map_err(|e| LibvirtError::Operation(format!("Failed to read XML of VM '{name}': {e}")))?;
    let disk = super::template_apply::primary_disk_path_from_xml(&xml).ok_or_else(|| {
        LibvirtError::Invalid(format!(
            "VM '{name}' has no file-backed primary disk to inject into"
        ))
    })?;
    if !disk.is_file() {
        return Err(LibvirtError::Invalid(format!(
            "Disk image {} for VM '{name}' is not a file on this host",
            disk.display()
        )));
    }

    let binary = resolve_guestkit_binary(cfg);
    if !binary.is_file() {
        return Err(LibvirtError::Invalid(format!(
            "GuestKit binary not found at {} — install GuestKit on this hypervisor or set [libvirt].guestkit_agent_binary",
            binary.display()
        )));
    }

    let mut cmd = Command::new("guestkit");
    cmd.arg("agent-inject")
        .arg(&disk)
        .arg("--agent-binary")
        .arg(&binary);
    if dry_run {
        cmd.arg("--dry-run");
    }
    let out = cmd
        .output()
        .map_err(|e| LibvirtError::Operation(format!("Failed to run guestkit: {e}")))?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(err.trim());
    }
    // Keep responses small and avoid echoing an unbounded tool log back to the browser.
    let output: String = text
        .trim()
        .chars()
        .rev()
        .take(4000)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if !out.status.success() {
        return Err(LibvirtError::Operation(format!(
            "guestkit agent-inject failed for VM '{name}': {output}"
        )));
    }
    Ok(AgentInjectReport {
        vm: name.to_string(),
        disk: disk.display().to_string(),
        binary: binary.display().to_string(),
        dry_run,
        output,
    })
}

/// Whether the GuestKit pieces Machina shells out to are present on this hypervisor.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GuestkitStatus {
    /// `guestkit` CLI found on PATH (needed for offline diagnose/repair/inject).
    pub cli_found: bool,
    pub cli_path: Option<String>,
    /// Agent binary that gets injected into guests (`[libvirt].guestkit_agent_binary`).
    pub agent_binary: String,
    pub agent_binary_found: bool,
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

pub fn guestkit_status(cfg: Option<&LibvirtConfig>) -> GuestkitStatus {
    let cli = find_in_path("guestkit");
    let agent = resolve_guestkit_binary(cfg);
    GuestkitStatus {
        cli_found: cli.is_some(),
        cli_path: cli.map(|p| p.display().to_string()),
        agent_binary_found: agent.is_file(),
        agent_binary: agent.display().to_string(),
    }
}
