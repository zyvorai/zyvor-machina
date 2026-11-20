// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct MigrationAdvisorReport {
    pub vm_name: String,
    pub provider: String,
    pub readiness_percent: u8,
    pub safe: Vec<String>,
    pub risks: Vec<String>,
    pub recommended_target: serde_json::Value,
    pub remediation: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guestkit_boot_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guestkit_migration_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guestkit_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firewall_dependencies: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firewall_migration_summary: Option<String>,
}

pub fn advise_vmware_vm(vm_name: &str, os_hint: &str, has_rdm: bool) -> MigrationAdvisorReport {
    let mut safe = vec![
        "Standard libvirt/KVM target supported".into(),
        "h2kvm offline path: GuestKit repair → qcow2 → libvirt (--emit-domain-xml)".into(),
    ];
    let mut risks = Vec::new();
    let mut remediation = Vec::new();
    let mut score: i32 = 85;

    if has_rdm {
        risks.push("RDM disk detected — may require storage conversion".into());
        remediation.push("Convert RDM to VMDK or map to shared storage pool".into());
        score -= 25;
    }
    if os_hint.to_lowercase().contains("windows") {
        safe.push("Windows — h2kvm injects VirtIO drivers offline; use UEFI template".into());
    } else {
        safe.push("Linux — virtio-scsi recommended".into());
    }
    if vm_name.is_empty() {
        score = 50;
        risks.push("VM name unknown — run scan first".into());
    }

    risks.push("Verify static IP and VMware Tools removal post-migrate".into());
    let vm = if vm_name.is_empty() { "<vm>" } else { vm_name };
    remediation.push(format!(
        "Convert with h2kvm: h2kvmctl --cmd vsphere --vs-vm {vm} --to-output {vm}.qcow2 --flatten --emit-domain-xml"
    ));
    remediation.push("Run preflight migrate check after import".into());

    MigrationAdvisorReport {
        vm_name: vm_name.into(),
        provider: "vmware".into(),
        readiness_percent: score.clamp(0, 100) as u8,
        safe,
        risks,
        recommended_target: serde_json::json!({
            "platform": "libvirt",
            "vcpu": 4,
            "memory_gib": 16,
            "disk_bus": "virtio-scsi",
            "network": "default"
        }),
        remediation,
        guestkit_boot_score: None,
        guestkit_migration_score: None,
        guestkit_summary: None,
        firewall_dependencies: None,
        firewall_migration_summary: None,
    }
}

pub fn merge_guestkit(
    mut report: MigrationAdvisorReport,
    boot_score: f64,
    migration_score: f64,
    blockers: &[String],
    warnings: &[String],
    summary: &str,
) -> MigrationAdvisorReport {
    report.guestkit_boot_score = Some(boot_score);
    report.guestkit_migration_score = Some(migration_score);
    report.guestkit_summary = Some(summary.into());

    let blended = (report.readiness_percent as f64 * 0.35 + migration_score * 0.65).round() as u8;
    report.readiness_percent = blended.clamp(0, 100);

    for b in blockers {
        report.risks.push(format!("GuestKit blocker: {b}"));
        report
            .remediation
            .push("Run GuestKit repair / migrate-plan export before cutover".into());
    }
    for w in warnings.iter().take(3) {
        report.risks.push(format!("GuestKit warning: {w}"));
    }
    if migration_score >= 80.0 {
        report.safe.push(format!(
            "GuestKit migration score {:.0}% on KVM target",
            migration_score
        ));
    }
    report
}

pub fn merge_firewall_migration(
    mut report: MigrationAdvisorReport,
    dependencies: Vec<String>,
) -> MigrationAdvisorReport {
    if dependencies.is_empty() {
        report.firewall_migration_summary = Some(
            "No observed firewall port dependencies — verify with native eBPF flows before cutover"
                .into(),
        );
        return report;
    }
    report.firewall_dependencies = Some(dependencies.clone());
    report.firewall_migration_summary = Some(format!(
        "Preserve inbound/outbound rules for: {}",
        dependencies.join(", ")
    ));
    report
        .remediation
        .push("Apply matching Zeus Firewall profile on target VM before migration cutover".into());
    for dep in dependencies.iter().take(5) {
        report.safe.push(format!("Firewall dependency: {dep}"));
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vmware_advice_points_at_h2kvm_not_v2v() {
        let r = advise_vmware_vm("web-01", "windows", false);
        let text = format!("{:?} {:?}", r.safe, r.remediation);
        assert!(!text.to_lowercase().contains("v2v"));
        assert!(r
            .remediation
            .iter()
            .any(|s| s.contains("h2kvmctl --cmd vsphere --vs-vm web-01 --to-output web-01.qcow2")));
        assert!(r.safe.iter().any(|s| s.contains("VirtIO drivers offline")));

        let unknown = advise_vmware_vm("", "linux", true);
        assert!(unknown
            .remediation
            .iter()
            .any(|s| s.contains("--vs-vm <vm>")));
        assert_eq!(unknown.readiness_percent, 50);
    }
}
