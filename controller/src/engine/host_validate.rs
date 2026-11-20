// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::agent_client;

#[derive(Debug, Clone, Serialize)]
pub struct ValidationCheck {
    pub name: String,
    pub passed: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostValidationReport {
    pub ok: bool,
    pub checks: Vec<ValidationCheck>,
}

pub async fn validate_host(
    pool: &DbPool,
    host_id: Uuid,
) -> anyhow::Result<HostValidationReport> {
    let mut checks = Vec::new();

    let row: Option<(String, String, String)> =
        crate::db::query_as("SELECT hostname, agent_grpc_addr, libvirt_uri FROM hosts WHERE id = ?")
            .bind(host_id)
            .fetch_optional(pool)
            .await?;

    let Some((hostname, agent_addr, libvirt_uri)) = row else {
        checks.push(fail(
            "host_exists",
            "Host not in inventory",
            "Re-enroll the host with a valid join token",
        ));
        return Ok(HostValidationReport { ok: false, checks });
    };
    checks.push(pass(
        "host_exists",
        &format!("Host '{hostname}' registered"),
    ));

    if libvirt_uri.trim().is_empty() {
        checks.push(fail(
            "libvirt_uri",
            "libvirt URI not configured",
            "Set libvirt_uri to qemu:///system on the host record",
        ));
    } else {
        checks.push(pass("libvirt_uri", &format!("URI {libvirt_uri}")));
    }

    match agent_client::connect(&agent_addr).await {
        Ok(mut client) => {
            checks.push(pass("agent_reachable", &format!("Agent at {agent_addr}")));

            match agent_client::get_host_info(&mut client).await {
                Ok(info) => {
                    if info.libvirt_version.is_empty() {
                        checks.push(fail(
                            "libvirt_running",
                            "libvirt version unknown",
                            "Ensure libvirtd is running: systemctl start libvirtd",
                        ));
                    } else {
                        checks.push(pass(
                            "libvirt_running",
                            &format!("libvirt {}", info.libvirt_version),
                        ));
                    }
                    if info.qemu_version.is_empty() {
                        checks.push(fail(
                            "qemu_installed",
                            "QEMU binary not detected",
                            "Install qemu-kvm and ensure /usr/bin/qemu-system-x86_64 or qemu-kvm exists",
                        ));
                    } else {
                        checks.push(pass(
                            "qemu_installed",
                            &format!("QEMU {}", info.qemu_version),
                        ));
                    }
                    if info.cpu_model.is_empty() {
                        checks.push(warn(
                            "cpu_model",
                            "CPU model not reported",
                            "Verify /proc/cpuinfo on the host",
                        ));
                    } else {
                        checks.push(pass("cpu_model", &info.cpu_model));
                    }
                }
                Err(e) => {
                    checks.push(fail(
                        "host_info",
                        &format!("Could not read host info: {e}"),
                        "Check agent logs: journalctl -u machina-agent",
                    ));
                }
            }

            match agent_client::heartbeat(&mut client, &host_id.to_string()).await {
                Ok(hb) => {
                    checks.push(pass(
                        "heartbeat",
                        &format!("{} VMs, CPU {:.0}%", hb.vm_count, hb.cpu_percent),
                    ));
                }
                Err(e) => {
                    checks.push(fail(
                        "heartbeat",
                        &format!("Heartbeat failed: {e}"),
                        "Restart machina-agent and verify gRPC port 50051",
                    ));
                }
            }
        }
        Err(e) => {
            checks.push(fail(
                "agent_reachable",
                &format!("Cannot connect to agent: {e}"),
                "Verify machina-agent is running and firewall allows port 50051",
            ));
        }
    }

    // Migration port hint (libvirt default 49152-49215)
    checks.push(warn(
        "migrate_ports",
        "Ensure libvirt migration ports (49152-49215/tcp) are open between cluster hosts",
        "Open firewalld/iptables for migration on all KVM nodes",
    ));

    let ok = checks.iter().all(|c| c.passed);
    Ok(HostValidationReport { ok, checks })
}

pub async fn persist_validation(
    pool: &DbPool,
    host_id: Uuid,
    report: &HostValidationReport,
) -> anyhow::Result<()> {
    let status = if report.ok { "passed" } else { "failed" };
    let state = if report.ok {
        "online"
    } else {
        "pending_validation"
    };
    crate::db::query(
        "UPDATE hosts SET validation_status = ?, validation_report = ?, state = ? WHERE id = ?",
    )
    .bind(status)
    .bind(serde_json::to_value(&report.checks)?)
    .bind(state)
    .bind(host_id)
    .execute(pool)
    .await?;
    Ok(())
}

fn pass(name: &str, message: &str) -> ValidationCheck {
    ValidationCheck {
        name: name.into(),
        passed: true,
        message: message.into(),
        remediation: None,
    }
}

fn warn(name: &str, message: &str, remediation: &str) -> ValidationCheck {
    ValidationCheck {
        name: name.into(),
        passed: true,
        message: message.into(),
        remediation: Some(remediation.into()),
    }
}

fn fail(name: &str, message: &str, remediation: &str) -> ValidationCheck {
    ValidationCheck {
        name: name.into(),
        passed: false,
        message: message.into(),
        remediation: Some(remediation.into()),
    }
}
