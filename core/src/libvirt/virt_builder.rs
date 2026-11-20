// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Optional libguestfs integration: `virt-builder` disk images, optional `virt-customize` / `virt-sysprep`.
//! Root passwords are passed only as `--root-password file:…` (never `password:` on the process argv).
//!
//! Template discovery prefers `virt-builder --list --list-format json` (see libguestfs docs); falls back to plain `--list`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use virt::connect::Connect;

use crate::config::LibvirtConfig;
use crate::state::CreateVmRequest;
use crate::LibvirtError;

use super::subprocess::{self, VmCreateLogSink};

fn run_guestfs_tool(
    tool: &str,
    args: &[String],
    log: Option<&VmCreateLogSink>,
) -> Result<(), LibvirtError> {
    let summary = format!("$ {tool} {}", args.join(" "));
    let mut cmd = Command::new(tool);
    cmd.args(args);
    let out = subprocess::run_command_streaming(cmd, &summary, tool, log)?;
    if !out.status.success() {
        return Err(LibvirtError::Operation(format!(
            "{tool} failed (exit {}); see streamed log",
            out.status
        )));
    }
    Ok(())
}

/// Whether `virt-builder` runs and `--version` succeeds (guestfs-tools installed).
pub fn virt_builder_installed() -> bool {
    Command::new("virt-builder")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// First line of `virt-builder --version` (for support / UI diagnostics).
pub fn virt_builder_version_line() -> Result<String, LibvirtError> {
    let out = Command::new("virt-builder")
        .arg("--version")
        .output()
        .map_err(|e| {
            LibvirtError::Operation(format!(
                "virt-builder not found or not executable (install guestfs-tools / libguestfs-tools): {e}"
            ))
        })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(LibvirtError::Operation(format!(
            "virt-builder --version failed: {stderr}"
        )));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return Err(LibvirtError::Operation(
            "virt-builder --version produced empty output".into(),
        ));
    }
    Ok(line.to_string())
}

/// `virt-builder --notes <template>` (template-specific caveats).
pub fn template_notes(template: &str) -> Result<String, LibvirtError> {
    let t = template.trim();
    crate::validate::validate_virt_builder_os(t)?;
    let out = Command::new("virt-builder")
        .args(["--notes", t])
        .output()
        .map_err(|e| {
            LibvirtError::Operation(format!(
                "Failed to run virt-builder --notes (is virt-builder installed?): {e}"
            ))
        })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(LibvirtError::Operation(format!(
            "virt-builder --notes failed: {stderr}"
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// If `req.virt_builder_os` is set, run `virt-builder` (and optional customize / sysprep), then set `existing_disk`.
pub fn materialize_virt_builder_if_requested(
    conn: &Connect,
    req: &mut CreateVmRequest,
    cfg: &LibvirtConfig,
    log: Option<&VmCreateLogSink>,
) -> Result<(), LibvirtError> {
    let os = req.virt_builder_os.trim();
    if os.is_empty() {
        return Ok(());
    }
    crate::validate::validate_virt_builder_os(os)?;

    if !cfg.virt_builder_allowed {
        return Err(LibvirtError::Invalid(
            "virt-builder is disabled on this server ([libvirt] virt_builder_allowed = false)"
                .into(),
        ));
    }

    if !req.existing_disk.trim().is_empty() {
        return Err(LibvirtError::Invalid(
            "virt_builder_os cannot be used when a root disk is already set (existing_disk or golden template)"
                .into(),
        ));
    }

    if !req.iso.trim().is_empty() {
        return Err(LibvirtError::Invalid(
            "virt_builder_os produces a full root disk; omit install ISO (`iso`)".into(),
        ));
    }

    let hostname = resolve_hostname(req)?;

    let pk = req.virt_builder_ssh_pubkey.trim();
    let pw_file = req.virt_builder_root_password_file.trim();

    let def_path = cfg.virt_builder_default_ssh_pubkey_path.trim();
    let default_pubkey_file = if !def_path.is_empty() {
        let p = Path::new(def_path);
        if p.is_file() {
            Some(p.to_path_buf())
        } else {
            None
        }
    } else {
        None
    };

    if pk.is_empty() && pw_file.is_empty() && default_pubkey_file.is_none() {
        return Err(LibvirtError::Invalid(
            "virt_builder: set virt_builder_ssh_pubkey, or virt_builder_root_password_file, or [libvirt] virt_builder_default_ssh_pubkey_path to an existing file"
                .into(),
        ));
    }

    crate::validate::validate_disk_gb(req.disk_gb)?;

    let dest = super::create::find_disk_path(conn, &req.name)?;
    if Path::new(&dest).exists() {
        return Err(LibvirtError::Operation(format!(
            "Refusing to overwrite existing disk: {dest}"
        )));
    }

    let dest_pb = Path::new(&dest);
    let parent = dest_pb
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            LibvirtError::Invalid("Could not determine parent directory for new disk".into())
        })?;
    let parent_canon = parent
        .canonicalize()
        .map_err(|e| LibvirtError::Invalid(format!("Output parent directory inaccessible: {e}")))?;
    crate::build_precheck::precheck_virt_builder_host_env(
        &parent_canon,
        cfg.virt_image_build_min_free_parent_bytes,
        cfg.virt_image_build_min_free_tmp_bytes,
    )?;

    let mut args: Vec<String> = vec![
        os.to_string(),
        "--format".into(),
        "qcow2".into(),
        "--size".into(),
        format!("{}G", req.disk_gb),
        "-o".into(),
        dest.clone(),
        "--hostname".into(),
        hostname,
    ];

    if cfg.virt_builder_update {
        args.push("--update".into());
    }

    if req.virt_builder_selinux_relabel {
        args.push("--selinux-relabel".into());
    }

    let all_packages: Vec<&str> = cfg
        .virt_builder_default_packages
        .iter()
        .map(|s| s.as_str())
        .chain(req.virt_builder_packages.iter().map(|s| s.as_str()))
        .collect();
    for pkg in &all_packages {
        let p = pkg.trim();
        if !p.is_empty() {
            args.push("--install".into());
            args.push(p.to_string());
        }
    }

    for cmd in &req.virt_builder_firstboot_commands {
        let c = cmd.trim();
        if !c.is_empty() {
            args.push("--firstboot-command".into());
            args.push(c.to_string());
        }
    }

    let mut tmp_key: Option<TempSshKeyFile> = None;
    if !pk.is_empty() {
        tmp_key = Some(TempSshKeyFile::write_line(pk)?);
        let p = tmp_key.as_ref().unwrap().path();
        args.push("--ssh-inject".into());
        args.push(format!("root:file:{}", p.display()));
    } else if let Some(ref p) = default_pubkey_file {
        args.push("--ssh-inject".into());
        args.push(format!("root:file:{}", p.display()));
    }

    if !pw_file.is_empty() {
        let pb = Path::new(pw_file)
            .canonicalize()
            .map_err(|e| LibvirtError::Invalid(format!("virt_builder_root_password_file: {e}")))?;
        args.push("--root-password".into());
        args.push(format!("file:{}", pb.display()));
    }

    tracing::info!("virt-builder {} (output {})", os, dest);

    let summary = format!("$ virt-builder {}", args.join(" "));
    let mut vb = Command::new("virt-builder");
    vb.args(&args);
    let out = subprocess::run_command_streaming(vb, &summary, "virt-builder", log)?;

    drop(tmp_key);

    if !out.status.success() {
        let _ = fs::remove_file(&dest);
        return Err(LibvirtError::Operation(format!(
            "virt-builder failed (exit {}); see streamed log",
            out.status
        )));
    }

    super::guest_agent_provision::inject_guestkit_into_disk(&dest, Some(cfg), log)?;

    let disk = Path::new(&dest);
    post_customize(disk, req, log)?;
    if req.virt_builder_sysprep {
        tracing::info!("virt-sysprep {}", dest);
        run_guestfs_tool("virt-sysprep", &["-a".into(), dest.clone()], log).inspect_err(|_e| {
            let _ = fs::remove_file(&dest);
        })?;
    }

    req.existing_disk = dest;
    Ok(())
}

fn post_customize(
    disk: &Path,
    req: &CreateVmRequest,
    log: Option<&VmCreateLogSink>,
) -> Result<(), LibvirtError> {
    let installs: Vec<String> = req
        .virt_builder_post_customize_install
        .iter()
        .filter_map(|s| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        })
        .collect();
    let runs: Vec<String> = req
        .virt_builder_post_customize_run
        .iter()
        .filter_map(|s| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        })
        .collect();

    if installs.is_empty() && runs.is_empty() {
        return Ok(());
    }

    let mut args: Vec<String> = vec!["-a".into(), disk.display().to_string()];
    for p in &installs {
        args.push("--install".into());
        args.push(p.clone());
    }
    for c in &runs {
        args.push("--run-command".into());
        args.push(c.clone());
    }

    tracing::info!("virt-customize on {}", disk.display());
    run_guestfs_tool("virt-customize", &args, log).inspect_err(|_e| {
        let _ = fs::remove_file(disk);
    })
}

fn resolve_hostname(req: &CreateVmRequest) -> Result<String, LibvirtError> {
    let h = req.virt_builder_hostname.trim();
    if !h.is_empty() {
        crate::validate::validate_virt_builder_hostname(h)?;
        return Ok(h.to_string());
    }
    let derived = req.name.replace('_', "-");
    crate::validate::validate_virt_builder_hostname(&derived)?;
    Ok(derived)
}

struct TempSshKeyFile {
    path: PathBuf,
}

impl TempSshKeyFile {
    fn write_line(pubkey_line: &str) -> Result<Self, LibvirtError> {
        let mut rng = rand::thread_rng();
        let path = std::env::temp_dir().join(format!(
            "machina-vb-{}-{}.pub",
            std::process::id(),
            rng.gen::<u64>()
        ));
        fs::write(&path, format!("{}\n", pubkey_line.trim())).map_err(|e| {
            LibvirtError::Operation(format!("Failed to write temp SSH pubkey file: {e}"))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&path)
                .map_err(|e| LibvirtError::Operation(e.to_string()))?
                .permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&path, perms)
                .map_err(|e| LibvirtError::Operation(e.to_string()))?;
        }
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempSshKeyFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// One row from `virt-builder --list --list-format json` (best-effort fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtBuilderTemplateInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
}

/// Parsed template index for APIs / UIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtBuilderIndex {
    /// `version` field from virt-builder JSON, or `0` when parsed from plain `--list`.
    pub format_version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_uri: Option<String>,
    pub items: Vec<VirtBuilderTemplateInfo>,
}

fn list_builder_templates_text() -> Result<Vec<String>, LibvirtError> {
    let out = Command::new("virt-builder")
        .arg("--list")
        .output()
        .map_err(|e| {
            LibvirtError::Operation(format!(
                "Failed to run virt-builder --list (is virt-builder installed?): {e}"
            ))
        })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(LibvirtError::Operation(format!(
            "virt-builder --list failed: {stderr}"
        )));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut v = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let token = line.split_whitespace().next().unwrap_or(line);
        if !token.is_empty() {
            v.push(token.to_string());
        }
    }
    v.sort();
    v.dedup();
    Ok(v)
}

fn push_template_from_json_value(items: &mut Vec<VirtBuilderTemplateInfo>, entry: &Value) {
    match entry {
        Value::String(name) => {
            let name = name.trim();
            if !name.is_empty() {
                items.push(VirtBuilderTemplateInfo {
                    name: name.to_string(),
                    summary: None,
                    arch: None,
                    size: None,
                });
            }
        }
        Value::Object(map) => {
            let name = map
                .get("name")
                .or_else(|| map.get("os-version"))
                .or_else(|| map.get("os"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if name.is_empty() {
                return;
            }
            let summary = map
                .get("full_version")
                .or_else(|| map.get("full-version"))
                .or_else(|| map.get("notes"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from);
            let arch = map
                .get("arch")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from);
            let size = map
                .get("size")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from);
            items.push(VirtBuilderTemplateInfo {
                name: name.to_string(),
                summary,
                arch,
                size,
            });
        }
        _ => {}
    }
}

fn index_from_json_value(value: &Value) -> VirtBuilderIndex {
    let version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    let mut items = Vec::new();
    let mut source_uri = None;

    if let Some(arr) = value.get("templates").and_then(|a| a.as_array()) {
        for entry in arr {
            push_template_from_json_value(&mut items, entry);
        }
    }

    if let Some(sources) = value.get("sources").and_then(|s| s.as_array()) {
        for src in sources {
            if source_uri.is_none() {
                source_uri = src
                    .get("uri")
                    .or_else(|| src.get("URL"))
                    .and_then(|u| u.as_str())
                    .map(String::from);
            }
            if let Some(arr) = src.get("templates").and_then(|a| a.as_array()) {
                for entry in arr {
                    push_template_from_json_value(&mut items, entry);
                }
            }
        }
    }

    items.sort_by(|a, b| a.name.cmp(&b.name));
    items.dedup_by(|a, b| a.name == b.name);

    VirtBuilderIndex {
        format_version: version,
        source_uri,
        items,
    }
}

/// `virt-builder --list --list-format json` when available, else plain `--list` as name-only rows.
pub fn list_builder_index() -> Result<VirtBuilderIndex, LibvirtError> {
    let json_out = Command::new("virt-builder")
        .args(["--list", "--list-format", "json"])
        .output()
        .map_err(|e| {
            LibvirtError::Operation(format!(
                "Failed to run virt-builder --list --list-format json: {e}"
            ))
        })?;

    if json_out.status.success() {
        if let Ok(v) = serde_json::from_slice::<Value>(&json_out.stdout) {
            let idx = index_from_json_value(&v);
            if !idx.items.is_empty() {
                return Ok(idx);
            }
        }
    }

    let names = list_builder_templates_text()?;
    Ok(VirtBuilderIndex {
        format_version: 0,
        source_uri: None,
        items: names
            .into_iter()
            .map(|name| VirtBuilderTemplateInfo {
                name,
                summary: None,
                arch: None,
                size: None,
            })
            .collect(),
    })
}

/// Template names only (backward compatible).
pub fn list_builder_templates() -> Result<Vec<String>, LibvirtError> {
    Ok(list_builder_index()?
        .items
        .into_iter()
        .map(|i| i.name)
        .collect())
}

#[cfg(test)]
mod index_tests {
    use super::*;

    #[test]
    fn parse_json_templates_strings() {
        let v: Value =
            serde_json::from_str(r#"{"version":1,"templates":["debian-13","fedora-44"]}"#).unwrap();
        let idx = index_from_json_value(&v);
        assert_eq!(idx.format_version, 1);
        assert_eq!(idx.items.len(), 2);
        assert_eq!(idx.items[0].name, "debian-13");
    }

    #[test]
    fn parse_json_sources_objects() {
        let raw = r#"{
            "version": 2,
            "sources": [
                {
                    "uri": "https://example.invalid/index",
                    "templates": [
                        {"name": "ubuntu-22.04", "arch": "x86_64", "full_version": "Ubuntu 22.04 LTS", "size": "3G"}
                    ]
                }
            ]
        }"#;
        let v: Value = serde_json::from_str(raw).unwrap();
        let idx = index_from_json_value(&v);
        assert_eq!(idx.format_version, 2);
        assert_eq!(
            idx.source_uri.as_deref(),
            Some("https://example.invalid/index")
        );
        let u = idx.items.iter().find(|i| i.name == "ubuntu-22.04").unwrap();
        assert_eq!(u.arch.as_deref(), Some("x86_64"));
        assert!(u.summary.as_ref().unwrap().contains("Ubuntu"));
    }
}
