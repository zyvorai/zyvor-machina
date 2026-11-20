// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Optional `kubectl` / `virtctl` execution for KubeVirt migration ([`machina_core::config::KubeVirtConfig`]).
//! Disabled unless `[kubevirt] exec_enabled = true`.

use std::path::Path;
use tokio::process::Command;

use machina_core::config::KubeVirtConfig;
use machina_core::LibvirtError;

async fn apply_kubeconfig(cmd: &mut Command, k: &KubeVirtConfig) {
    let p = k.kubeconfig_path.trim();
    if !p.is_empty() {
        cmd.env("KUBECONFIG", p);
    } else if let Some(path) = crate::k8s_kubeconfig::kubectl_kubeconfig_choice()
        .await
        .auto_selected_path
    {
        cmd.env("KUBECONFIG", path);
    }
}

/// Run `kubectl apply -f <yaml>` on the daemon host.
pub async fn kubectl_apply_yaml(
    k: &KubeVirtConfig,
    yaml_path: &Path,
) -> Result<(i32, String, String), LibvirtError> {
    if !k.exec_enabled {
        return Err(LibvirtError::Forbidden(
            "kubevirt.exec_enabled is false; set it true in machina config to allow cluster commands.".into(),
        ));
    }
    let bin = k.kubectl_binary.trim();
    if bin.is_empty() {
        return Err(LibvirtError::Invalid(
            "kubevirt.kubectl_binary is empty".into(),
        ));
    }
    let mut cmd = Command::new(bin);
    cmd.arg("apply").arg("-f").arg(yaml_path);
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    apply_kubeconfig(&mut cmd, k).await;
    let out = cmd
        .output()
        .await
        .map_err(|e| LibvirtError::Operation(format!("kubectl: failed to spawn: {e}")))?;
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    Ok((code, stdout, stderr))
}

/// Run `virtctl image-upload` for the libvirt disk into the upload DataVolume.
pub async fn virtctl_image_upload_disk(
    k: &KubeVirtConfig,
    dv_name: &str,
    size_gi: u32,
    image_path: &str,
    namespace: &str,
) -> Result<(i32, String, String), LibvirtError> {
    if !k.exec_enabled {
        return Err(LibvirtError::Forbidden(
            "kubevirt.exec_enabled is false; set it true in machina config to allow cluster commands.".into(),
        ));
    }
    let bin = k.virtctl_binary.trim();
    if bin.is_empty() {
        return Err(LibvirtError::Invalid(
            "kubevirt.virtctl_binary is empty".into(),
        ));
    }
    let timeout_m = k.upload_timeout_minutes.max(1);
    let mut cmd = Command::new(bin);
    cmd.args([
        "image-upload",
        "dv",
        dv_name,
        "--size",
        &format!("{size_gi}Gi"),
        "--image-path",
        image_path,
        "-n",
        namespace,
        "--insecure",
        "--upload-image-timeout",
        &format!("{timeout_m}m"),
    ]);
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    apply_kubeconfig(&mut cmd, k).await;
    let out = cmd.output().await.map_err(|e| {
        LibvirtError::Operation(format!("virtctl image-upload: failed to spawn: {e}"))
    })?;
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    Ok((code, stdout, stderr))
}

/// Run `virtctl start` for the KubeVirt VM.
pub async fn virtctl_start_vm(
    k: &KubeVirtConfig,
    vm_name: &str,
    namespace: &str,
) -> Result<(i32, String, String), LibvirtError> {
    if !k.exec_enabled {
        return Err(LibvirtError::Forbidden(
            "kubevirt.exec_enabled is false; set it true in machina config to allow cluster commands.".into(),
        ));
    }
    let bin = k.virtctl_binary.trim();
    if bin.is_empty() {
        return Err(LibvirtError::Invalid(
            "kubevirt.virtctl_binary is empty".into(),
        ));
    }
    let mut cmd = Command::new(bin);
    cmd.args(["start", vm_name, "-n", namespace]);
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    apply_kubeconfig(&mut cmd, k).await;
    let out = cmd
        .output()
        .await
        .map_err(|e| LibvirtError::Operation(format!("virtctl start: failed to spawn: {e}")))?;
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    Ok((code, stdout, stderr))
}
