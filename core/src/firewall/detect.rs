// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::path::Path;
use std::process::Command;

use super::types::FirewallBackend;

pub fn find_bin(name: &str) -> String {
    for dir in std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .chain(["/usr/sbin", "/sbin", "/usr/bin", "/bin"].iter().copied())
    {
        let p = Path::new(dir).join(name);
        if p.is_file() {
            return p.to_string_lossy().into_owned();
        }
    }
    name.to_string()
}

pub fn detect_backend() -> FirewallBackend {
    if let Ok(output) = Command::new(find_bin("ufw")).arg("status").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("Status:") {
                return FirewallBackend::Ufw;
            }
        }
    }
    if let Ok(output) = Command::new(find_bin("firewall-cmd"))
        .arg("--state")
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().eq_ignore_ascii_case("running") {
                return FirewallBackend::Firewalld;
            }
        }
    }
    if let Ok(output) = Command::new(find_bin("nft"))
        .args(["list", "ruleset"])
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            return FirewallBackend::Nftables;
        }
    }
    FirewallBackend::Iptables
}

pub fn run_cmd(bin: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(find_bin(bin))
        .args(args)
        .output()
        .map_err(|e| format!("failed to run {bin}: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{bin} failed: {stderr}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
