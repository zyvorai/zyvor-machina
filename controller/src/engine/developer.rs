// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Developer ecosystem — schema export for Terraform + SDK (Phase 30).

use machina_spec::VirtualMachine;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DeveloperOverview {
    pub openapi_url: String,
    pub sdk_typescript: SdkPackageInfo,
    pub terraform: TerraformInfo,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SdkPackageInfo {
    pub path: String,
    pub version: String,
    pub install: String,
    pub resources: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TerraformInfo {
    pub provider_source: String,
    pub examples_path: String,
    pub resources: Vec<TerraformResourceSchema>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TerraformResourceSchema {
    pub name: String,
    pub kind: String,
    pub api_path: String,
    pub attributes: Vec<String>,
}

pub fn overview() -> DeveloperOverview {
    DeveloperOverview {
        openapi_url: "/api/v1/openapi.json".into(),
        sdk_typescript: SdkPackageInfo {
            path: "sdk/typescript".into(),
            version: "0.1.0".into(),
            install: "npm install ../sdk/typescript".into(),
            resources: vec![
                "listHosts".into(),
                "listVms".into(),
                "createVm".into(),
                "getOperationsOverview".into(),
                "getObservabilityOverview".into(),
            ],
        },
        terraform: TerraformInfo {
            provider_source: "zyvor/machina".into(),
            examples_path: "terraform/machina/examples".into(),
            resources: terraform_schemas(),
        },
        summary: "TypeScript SDK + Terraform schemas aligned to /api/v1 (GA v1)".into(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VmExportBundle {
    pub vm_id: String,
    pub vm_name: String,
    pub terraform: String,
    pub ansible_role: String,
    pub cloud_init: String,
    pub domain_xml: String,
}

pub async fn export_vm_bundle(
    pool: &crate::db::DbPool,
    agent_addr: &str,
    vm_id: uuid::Uuid,
) -> anyhow::Result<VmExportBundle> {
    // Inventory rows may have empty/`{}` spec_json (imported/libvirt-synced VMs).
    // Fall back to columnar vcpus/memory/project so IaC export still works.
    let row: (String, Option<String>, i32, i64, serde_json::Value) =
        crate::db::query_as("SELECT name, project, vcpus, memory_mib, spec_json FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let (name, project_col, vcpus_col, memory_col, spec_val) = row;

    let (vcpus, memory_mib, project, cloud_init) = match serde_json::from_value::<VirtualMachine>(
        spec_val,
    ) {
        Ok(vm) => {
            let cloud_init = vm
                    .spec
                    .cloud_init
                    .as_ref()
                    .map(|c| {
                        format!(
                            "#cloud-config\nusers:\n  - name: {}\n    ssh_authorized_keys:\n      - {}\n",
                            c.user,
                            c.ssh_pubkey.as_deref().unwrap_or("")
                        )
                    })
                    .unwrap_or_else(|| "#cloud-config\n# (no cloud-init in spec)\n".into());
            (
                vm.total_vcpus(),
                vm.memory_mib().unwrap_or(memory_col as u64),
                vm.metadata
                    .project
                    .unwrap_or_else(|| project_col.unwrap_or_else(|| "default".into())),
                cloud_init,
            )
        }
        Err(_) => (
            vcpus_col.max(1) as u32,
            memory_col.max(128) as u64,
            project_col.unwrap_or_else(|| "default".into()),
            "#cloud-config\n# (no cloud-init in spec)\n".into(),
        ),
    };

    let mut client = crate::agent_client::connect(agent_addr).await?;
    let domain_xml = crate::agent_client::get_domain_xml(&mut client, &name).await?;
    let terraform = format!(
        r#"resource "machina_vm" "{name}" {{
  name         = "{name}"
  vcpus        = {vcpus}
  memory_mib   = {memory_mib}
  desired_state = "running"
  project      = "{project}"
}}
"#
    );

    let ansible_role = format!(
        r#"- name: Configure {name}
  hosts: localhost
  tasks:
    - name: Ensure VM is registered in Machina
      debug:
        msg: "Deploy via machina-controller POST /api/v1/vms"
"#
    );

    Ok(VmExportBundle {
        vm_id: vm_id.to_string(),
        vm_name: name,
        terraform,
        ansible_role,
        cloud_init,
        domain_xml,
    })
}

/// Zip bundle with terraform.tf, ansible, cloud-init, domain.xml, and manifest.json.
pub async fn export_vm_bundle_zip(
    pool: &crate::db::DbPool,
    agent_addr: &str,
    vm_id: uuid::Uuid,
) -> anyhow::Result<(String, Vec<u8>)> {
    let bundle = export_vm_bundle(pool, agent_addr, vm_id).await?;
    let name = bundle.vm_name.clone();
    let manifest = serde_json::to_string_pretty(&bundle)?;
    let files = [
        ("terraform.tf".into(), bundle.terraform.into_bytes()),
        ("ansible-role.txt".into(), bundle.ansible_role.into_bytes()),
        ("cloud-init.txt".into(), bundle.cloud_init.into_bytes()),
        ("domain.xml".into(), bundle.domain_xml.into_bytes()),
        ("manifest.json".into(), manifest.into_bytes()),
    ];
    let buf = crate::zip_write::deflated_zip(files)?;
    Ok((name, buf))
}

pub fn terraform_schemas() -> Vec<TerraformResourceSchema> {
    vec![
        TerraformResourceSchema {
            name: "machina_vm".into(),
            kind: "resource".into(),
            api_path: "POST /api/v1/vms".into(),
            attributes: vec![
                "name".into(),
                "vcpus".into(),
                "memory_mib".into(),
                "desired_state".into(),
                "project".into(),
            ],
        },
        TerraformResourceSchema {
            name: "machina_host".into(),
            kind: "data".into(),
            api_path: "GET /api/v1/hosts".into(),
            attributes: vec!["id".into(), "hostname".into(), "state".into()],
        },
        TerraformResourceSchema {
            name: "machina_storage_pool".into(),
            kind: "resource".into(),
            api_path: "POST /api/v1/storage/pools".into(),
            attributes: vec!["name".into(), "path".into(), "capacity_gib".into()],
        },
        TerraformResourceSchema {
            name: "machina_network".into(),
            kind: "resource".into(),
            api_path: "POST /api/v1/networks".into(),
            attributes: vec!["name".into(), "vlan_id".into(), "bridge".into()],
        },
    ]
}
