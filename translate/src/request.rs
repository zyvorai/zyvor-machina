// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use machina_spec::VirtualMachine;

/// Minimal CreateVmRequest-shaped payload for the existing libvirt create path.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TranslatedVmRequest {
    pub name: String,
    pub vcpus: u32,
    pub memory_mb: u64,
    pub disk_gb: u64,
    pub network: String,
    pub firmware: String,
    pub graphics_type: String,
    pub graphics_listen: String,
    pub existing_disk: String,
    pub iso: String,
    pub cloud_init_iso: String,
}

pub fn translate_vm(
    vm: &VirtualMachine,
    disk_path: &str,
) -> Result<TranslatedVmRequest, machina_spec::SpecError> {
    vm.validate()?;
    let network = vm
        .spec
        .network
        .first()
        .map(|n| n.network.clone())
        .unwrap_or_else(|| "default".into());
    Ok(TranslatedVmRequest {
        name: vm.metadata.name.clone(),
        vcpus: vm.total_vcpus(),
        memory_mb: vm.memory_mib()?,
        disk_gb: vm.root_disk_gib()?,
        network,
        firmware: vm.spec.firmware.clone(),
        graphics_type: vm.spec.graphics.r#type.clone(),
        graphics_listen: vm.spec.graphics.listen.clone(),
        existing_disk: disk_path.to_string(),
        iso: String::new(),
        cloud_init_iso: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use machina_spec::VirtualMachine;

    #[test]
    fn translate_basic_vm() {
        let vm = VirtualMachine::new("web-1", "4Gi");
        let req = translate_vm(&vm, "/var/lib/libvirt/images/web-1.qcow2").unwrap();
        assert_eq!(req.name, "web-1");
        assert_eq!(req.memory_mb, 4096);
        assert_eq!(req.existing_disk, "/var/lib/libvirt/images/web-1.qcow2");
    }
}
