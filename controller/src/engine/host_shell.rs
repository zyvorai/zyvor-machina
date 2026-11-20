// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Run shell commands on hypervisors. Loopback hosts run locally (no SSH).

use std::process::Output;
use std::time::Duration;

/// True when the controller and hypervisor are the same machine (seeded `127.0.0.1` / `localhost`).
pub fn is_local_hypervisor_address(address: &str) -> bool {
    let a = address.trim().to_lowercase();
    if a.is_empty() {
        return false;
    }
    a == "127.0.0.1" || a == "::1" || a == "localhost" || a.starts_with("127.")
}

/// Validate that `address` is a plain hostname/IP — no spaces, no dashes at start, no shell metacharacters.
/// Returns an error if the address could be interpreted as an SSH flag or shell injection.
fn validate_ssh_address(address: &str) -> anyhow::Result<()> {
    let a = address.trim();
    if a.is_empty() {
        anyhow::bail!("empty host address");
    }
    // Reject anything that starts with '-' (would be interpreted as an SSH option)
    // or contains shell metacharacters / whitespace.
    if a.starts_with('-')
        || a.contains(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    ';' | '&' | '|' | '$' | '`' | '(' | ')' | '<' | '>' | '"' | '\''
                )
        })
    {
        anyhow::bail!("invalid host address '{a}' — must be a plain hostname or IP");
    }
    Ok(())
}

pub async fn remote_file_exists(address: &str, path: &str) -> bool {
    let output = match run_remote(address, Duration::from_secs(6), &["test", "-f", path]).await {
        Ok(o) => o,
        Err(_) => return false,
    };
    output.status.success()
}

pub async fn run_remote_script(
    address: &str,
    timeout: Duration,
    script: &str,
) -> anyhow::Result<Output> {
    if !is_local_hypervisor_address(address) {
        validate_ssh_address(address)?;
    }
    let output = tokio::time::timeout(timeout, async {
        if is_local_hypervisor_address(address) {
            tokio::process::Command::new("bash")
                .args(["-lc", script])
                .output()
                .await
        } else {
            tokio::process::Command::new("ssh")
                .args([
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "ConnectTimeout=15",
                    "-o",
                    "StrictHostKeyChecking=accept-new",
                    address,
                    "bash",
                    "-lc",
                    script,
                ])
                .output()
                .await
        }
    })
    .await;

    match output {
        Ok(Ok(o)) => Ok(o),
        Ok(Err(e)) => Err(anyhow::anyhow!("{e}")),
        Err(_) => Err(anyhow::anyhow!("command timed out")),
    }
}

async fn run_remote(
    address: &str,
    timeout: Duration,
    remote_args: &[&str],
) -> anyhow::Result<Output> {
    if !is_local_hypervisor_address(address) {
        validate_ssh_address(address)?;
    }
    let output = tokio::time::timeout(timeout, async {
        if is_local_hypervisor_address(address) {
            tokio::process::Command::new(remote_args[0])
                .args(&remote_args[1..])
                .output()
                .await
        } else {
            let mut cmd = tokio::process::Command::new("ssh");
            cmd.args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=4",
                "-o",
                "StrictHostKeyChecking=accept-new",
                address,
            ]);
            cmd.args(remote_args);
            cmd.output().await
        }
    })
    .await;

    match output {
        Ok(Ok(o)) => Ok(o),
        Ok(Err(e)) => Err(anyhow::anyhow!("{e}")),
        Err(_) => Err(anyhow::anyhow!("command timed out")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_loopback_hosts() {
        assert!(is_local_hypervisor_address("127.0.0.1"));
        assert!(is_local_hypervisor_address("localhost"));
        assert!(is_local_hypervisor_address("127.0.0.2"));
        assert!(!is_local_hypervisor_address("10.0.0.2"));
        assert!(!is_local_hypervisor_address(""));
    }
}
