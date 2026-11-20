// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::subprocess::{self, VmCreateLogSink};
use crate::config::LibvirtConfig;
use crate::state::CreateVmRequest;
use crate::LibvirtError;

/// NoCloud `meta-data` as one line of JSON. JSON is also valid YAML, so cloud-init reads it as before, and minimal NoCloud
/// readers (cirros parses this file with a small awk JSON converter and fails on YAML with quoted values) accept it too.
pub fn nocloud_meta_data(instance_id: &str, hostname: &str) -> String {
    format!(
        "{}\n",
        serde_json::json!({ "instance-id": instance_id, "local-hostname": hostname })
    )
}

fn pick_seed_dir() -> PathBuf {
    // Prefer a common ISO pool location if present; otherwise fall back to machina-owned dir.
    let candidates = [
        "/data/iso/machina-cloud-init",
        "/var/lib/machina/cloud-init",
        "/var/tmp/machina-cloud-init",
    ];
    for c in candidates {
        let p = Path::new(c);
        if p.is_dir() {
            return p.to_path_buf();
        }
    }
    PathBuf::from(candidates[1])
}

fn ensure_dir(p: &Path) -> Result<(), LibvirtError> {
    fs::create_dir_all(p).map_err(|e| {
        LibvirtError::Operation(format!("create cloud-init dir {}: {e}", p.display()))
    })?;
    Ok(())
}

fn sanitize_vm_name_for_file(name: &str) -> String {
    // VM names are already validated elsewhere, but keep filenames conservative.
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn materialize_cloud_init_seed_if_requested(
    req: &mut CreateVmRequest,
    libvirt_cfg: &LibvirtConfig,
    log: Option<&VmCreateLogSink>,
) -> Result<(), LibvirtError> {
    if !req.cloud_init_iso.trim().is_empty() {
        return Ok(());
    }
    let mut user = req.cloud_init_user.trim().to_string();
    let pass = req.cloud_init_password.trim();
    let key = req.cloud_init_ssh_pubkey.trim();
    let guest_default = super::guest_agent_provision::guest_agent_enabled(Some(libvirt_cfg));
    if user.is_empty() && pass.is_empty() && key.is_empty() && !guest_default {
        return Ok(());
    }

    if user.is_empty() {
        if guest_default {
            user = super::guest_agent_provision::DEFAULT_CLOUD_INIT_USER.to_string();
            req.cloud_init_user = user.clone();
        } else {
            return Err(LibvirtError::Invalid(
                "cloud_init_user is required when using cloud-init automation".into(),
            ));
        }
    }
    if !user
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(LibvirtError::Invalid(
            "cloud_init_user has invalid characters".into(),
        ));
    }
    if !key.is_empty() && !key.starts_with("ssh-") {
        return Err(LibvirtError::Invalid(
            "cloud_init_ssh_pubkey must be a single-line OpenSSH public key".into(),
        ));
    }
    // The key and password are interpolated into the NoCloud user-data YAML; an
    // embedded newline lets a value break out of its block and inject top-level
    // cloud-config (e.g. `runcmd:`) that runs as root in the guest.
    if key.contains('\n') || key.contains('\r') {
        return Err(LibvirtError::Invalid(
            "cloud_init_ssh_pubkey must be a single line".into(),
        ));
    }
    if pass.contains('\n') || pass.contains('\r') {
        return Err(LibvirtError::Invalid(
            "cloud_init_password must not contain newlines".into(),
        ));
    }

    let seed_dir = pick_seed_dir();
    ensure_dir(&seed_dir)?;

    let safe = sanitize_vm_name_for_file(&req.name);
    let seed_iso = seed_dir.join(format!("{safe}-seed.iso"));
    let work_dir = seed_dir.join(format!("{safe}-seed.d"));
    ensure_dir(&work_dir)?;

    let user_data = work_dir.join("user-data");
    let meta_data = work_dir.join("meta-data");

    // NoCloud seed files
    let mut ud = String::new();
    ud.push_str("#cloud-config\n");
    ud.push_str(&format!("hostname: {}\n", req.name));
    ud.push_str(&format!("users:\n  - name: {}\n    sudo: ALL=(ALL) NOPASSWD:ALL\n    groups: [wheel, sudo]\n    shell: /bin/bash\n", user));
    if !key.is_empty() {
        ud.push_str("    ssh_authorized_keys:\n");
        ud.push_str(&format!("      - {}\n", key));
    }
    if !pass.is_empty() {
        ud.push_str("ssh_pwauth: true\n");
        ud.push_str("chpasswd:\n  expire: false\n  list: |\n");
        ud.push_str(&format!("    {}:{}\n", user, pass));
    }
    if !pass.is_empty() || !key.is_empty() {
        ud.push_str("package_update: true\n");
    }
    if guest_default {
        super::guest_agent_provision::append_guestkit_cloud_config(&mut ud, true);
    }
    if super::guest_agent_provision::vm_wants_graphical_desktop(&req.name) {
        super::guest_agent_provision::append_desktop_graphical_cloud_config(&mut ud, &user);
    }

    let md = nocloud_meta_data(&req.name, &req.name);

    fs::write(&user_data, ud)
        .map_err(|e| LibvirtError::Operation(format!("write {}: {e}", user_data.display())))?;
    fs::write(&meta_data, md)
        .map_err(|e| LibvirtError::Operation(format!("write {}: {e}", meta_data.display())))?;
    super::guest_agent_provision::stage_guestkit_seed_files(&work_dir, Some(libvirt_cfg), log)?;

    // Ensure qemu can read the seed.
    let _ = fs::set_permissions(&user_data, fs::Permissions::from_mode(0o644));
    let _ = fs::set_permissions(&meta_data, fs::Permissions::from_mode(0o644));

    // Create ISO (label must be cidata for NoCloud)
    let summary = format!(
        "$ genisoimage -output {} -volid cidata -joliet -rock {} {}",
        seed_iso.display(),
        user_data.display(),
        meta_data.display()
    );
    let mut cmd = Command::new("genisoimage");
    let guestkit_staged = work_dir.join("guestkit");
    if guestkit_staged.is_file() {
        cmd.args([
            "-output",
            seed_iso.to_string_lossy().as_ref(),
            "-volid",
            "cidata",
            "-joliet",
            "-rock",
            user_data.to_string_lossy().as_ref(),
            meta_data.to_string_lossy().as_ref(),
            guestkit_staged.to_string_lossy().as_ref(),
        ]);
    } else {
        cmd.args([
            "-output",
            seed_iso.to_string_lossy().as_ref(),
            "-volid",
            "cidata",
            "-joliet",
            "-rock",
            user_data.to_string_lossy().as_ref(),
            meta_data.to_string_lossy().as_ref(),
        ]);
    }
    let out = subprocess::run_command_streaming(cmd, &summary, "genisoimage", log)?;
    if !out.status.success() {
        return Err(LibvirtError::Operation(format!(
            "genisoimage failed (exit {}); see streamed log",
            out.status
        )));
    }

    let _ = fs::set_permissions(&seed_iso, fs::Permissions::from_mode(0o644));
    req.cloud_init_iso = seed_iso.to_string_lossy().to_string();
    subprocess::log_line(
        log,
        "machina",
        &format!("cloud-init seed ISO created: {}", req.cloud_init_iso),
    );
    Ok(())
}

#[cfg(test)]
mod meta_data_tests {
    use super::nocloud_meta_data;

    #[test]
    fn is_one_line_of_valid_json_that_round_trips() {
        let m = nocloud_meta_data("web-1", "web-1");
        assert_eq!(m.matches('\n').count(), 1, "one line, newline-terminated");
        let v: serde_json::Value = serde_json::from_str(m.trim()).unwrap();
        assert_eq!(v["instance-id"], "web-1");
        assert_eq!(v["local-hostname"], "web-1");
    }

    #[test]
    fn hostile_names_cannot_break_out_of_the_document() {
        let m = nocloud_meta_data("a\"b: c\nruncmd: [x]", "h'ost");
        let v: serde_json::Value = serde_json::from_str(m.trim()).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 2, "no extra keys");
        assert_eq!(v["instance-id"], "a\"b: c\nruncmd: [x]");
    }
}
