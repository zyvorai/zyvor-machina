// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::db::DbPool;
use uuid::Uuid;

struct CatalogTemplate {
    name: &'static str,
    version: &'static str,
    source_disk: &'static str,
    /// Public cloud image URL; fetched to `source_disk` on first template deploy when missing.
    download_url: Option<&'static str>,
    cloud_init: bool,
    os_family: &'static str,
    category: &'static str,
    workload: &'static str,
    description: &'static str,
    featured: bool,
    icon: &'static str,
}

fn catalog_firewall_profile(t: &CatalogTemplate) -> &'static str {
    match t.category {
        "Database" => "DatabaseServer",
        "Windows" => "ManagementNode",
        "Appliance" if t.name.contains("nginx") => "WebServer",
        "Appliance" => "LockedDown",
        _ if t.name.contains("rocky") || t.name.contains("alma") || t.name.contains("centos") => {
            "ProductionServer"
        }
        _ => "WebServer",
    }
}

/// Shared Ubuntu 24.04 golden image used by cloud-init appliance profiles (DB, proxy, VPN, etc.).
const UBUNTU_2404_DISK: &str = "/var/lib/libvirt/images/ubuntu-24.04.qcow2";

const CATALOG: &[CatalogTemplate] = &[
    CatalogTemplate {
        name: "ubuntu-24.04",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/ubuntu-24.04.qcow2",
        download_url: Some(
            "https://cloud-images.ubuntu.com/releases/24.04/release/ubuntu-24.04-server-cloudimg-amd64.img",
        ),
        cloud_init: true,
        os_family: "linux",
        category: "Linux",
        workload: "general",
        description: "Ubuntu 24.04 LTS — cloud-init, DHCP, ideal default for new VMs.",
        featured: true,
        icon: "🐧",
    },
    CatalogTemplate {
        name: "ubuntu-24.04-desktop",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/ubuntu-24.04-desktop.qcow2",
        download_url: None,
        cloud_init: true,
        os_family: "linux",
        category: "Linux",
        workload: "desktop",
        description: "Ubuntu 24.04 LTS GNOME desktop — built from server cloudimg + virt-customize; GDM autologin for VNC.",
        featured: true,
        icon: "🖥️",
    },
    CatalogTemplate {
        name: "ubuntu-25.10",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/ubuntu-25.10.qcow2",
        download_url: Some(
            "https://cloud-images.ubuntu.com/releases/25.10/release/ubuntu-25.10-server-cloudimg-amd64.img",
        ),
        cloud_init: true,
        os_family: "linux",
        category: "Linux",
        workload: "general",
        description: "Ubuntu 25.10 — current interim release with latest kernel and packages.",
        featured: true,
        icon: "🐧",
    },
    CatalogTemplate {
        name: "ubuntu-26.04",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/ubuntu-26.04.qcow2",
        download_url: Some(
            "https://cloud-images.ubuntu.com/releases/26.04/release/ubuntu-26.04-server-cloudimg-amd64.img",
        ),
        cloud_init: true,
        os_family: "linux",
        category: "Linux",
        workload: "general",
        description: "Ubuntu 26.04 LTS — Resolute Raccoon, latest LTS cloud-init image.",
        featured: true,
        icon: "🐧",
    },
    CatalogTemplate {
        name: "debian-13",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/debian-13.qcow2",
        download_url: Some(
            "https://cloud.debian.org/images/cloud/trixie/latest/debian-13-genericcloud-amd64.qcow2",
        ),
        cloud_init: true,
        os_family: "linux",
        category: "Linux",
        workload: "general",
        description: "Debian 13 Trixie — current stable, minimal cloud image.",
        featured: true,
        icon: "🐧",
    },
    CatalogTemplate {
        name: "centos-stream-10",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/centos-stream-10.qcow2",
        download_url: Some(
            "https://cloud.centos.org/centos/10-stream/x86_64/images/CentOS-Stream-GenericCloud-10-latest.x86_64.qcow2",
        ),
        cloud_init: true,
        os_family: "linux",
        category: "Linux",
        workload: "general",
        description: "CentOS Stream 10 — RHEL 10 upstream for Alma/Rocky-style hosts.",
        featured: true,
        icon: "🐧",
    },
    CatalogTemplate {
        name: "rocky-10",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/rocky-10.qcow2",
        download_url: Some(
            "https://dl.rockylinux.org/pub/rocky/10/images/x86_64/Rocky-10-GenericCloud-Base.latest.x86_64.qcow2",
        ),
        cloud_init: true,
        os_family: "linux",
        category: "Linux",
        workload: "general",
        description: "Rocky Linux 10 — enterprise Linux aligned with RHEL 10.",
        featured: true,
        icon: "🐧",
    },
    CatalogTemplate {
        name: "alma-10",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/alma-10.qcow2",
        download_url: Some(
            "https://repo.almalinux.org/almalinux/10/cloud/x86_64/images/AlmaLinux-10-GenericCloud-latest.x86_64.qcow2",
        ),
        cloud_init: true,
        os_family: "linux",
        category: "Linux",
        workload: "general",
        description: "AlmaLinux 10 — RHEL 10-compatible with cloud-init.",
        featured: true,
        icon: "🐧",
    },
    CatalogTemplate {
        name: "fedora-44",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/fedora-44.qcow2",
        download_url: Some(
            "https://download.fedoraproject.org/pub/fedora/linux/releases/44/Cloud/x86_64/images/Fedora-Cloud-Base-Generic-44-1.7.x86_64.qcow2",
        ),
        cloud_init: true,
        os_family: "linux",
        category: "Linux",
        workload: "general",
        description: "Fedora 44 — current Fedora with latest packages for dev and CI.",
        featured: true,
        icon: "🐧",
    },
    CatalogTemplate {
        name: "win11",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/win11.qcow2",
        download_url: None,
        cloud_init: false,
        os_family: "windows",
        category: "Windows",
        workload: "general",
        description: "Windows 11 — dockur/Podman Golden Forge (UEFI + VirtIO). Build via Create VM → Golden Forge, then clone or export to KubeVirt.",
        featured: true,
        icon: "🪟",
    },
    CatalogTemplate {
        name: "win10",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/win10.qcow2",
        download_url: None,
        cloud_init: false,
        os_family: "windows",
        category: "Windows",
        workload: "general",
        description: "Windows 10 — dockur/Podman Golden Forge (UEFI + VirtIO). Build via Create VM → Golden Forge, then clone or export to KubeVirt.",
        featured: true,
        icon: "🪟",
    },
    CatalogTemplate {
        name: "windows-server-2022",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/windows-server-2022.qcow2",
        download_url: None,
        cloud_init: false,
        os_family: "windows",
        category: "Windows",
        workload: "general",
        description: "Windows Server 2022 — dockur Golden Forge (UEFI + VirtIO). Build via Create VM → Golden Forge (guest id windows-server-2022).",
        featured: true,
        icon: "🪟",
    },
    CatalogTemplate {
        name: "windows-server-2025",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/windows-server-2025.qcow2",
        download_url: None,
        cloud_init: false,
        os_family: "windows",
        category: "Windows",
        workload: "general",
        description: "Windows Server 2025 — dockur Golden Forge (UEFI + VirtIO). Build via Create VM → Golden Forge (guest id windows-server-2025).",
        featured: true,
        icon: "🪟",
    },
    CatalogTemplate {
        name: "postgresql-16",
        version: "1.0.0",
        source_disk: UBUNTU_2404_DISK,
        download_url: None,
        cloud_init: true,
        os_family: "linux",
        category: "Database",
        workload: "database",
        description: "PostgreSQL 16 on Ubuntu — pre-tuned database appliance.",
        featured: true,
        icon: "🗄️",
    },
    CatalogTemplate {
        name: "mysql-8",
        version: "1.0.0",
        source_disk: UBUNTU_2404_DISK,
        download_url: None,
        cloud_init: true,
        os_family: "linux",
        category: "Database",
        workload: "database",
        description: "MySQL 8.0 — InnoDB, replication-ready base image.",
        featured: false,
        icon: "🗄️",
    },
    CatalogTemplate {
        name: "mariadb-11",
        version: "1.0.0",
        source_disk: UBUNTU_2404_DISK,
        download_url: None,
        cloud_init: true,
        os_family: "linux",
        category: "Database",
        workload: "database",
        description: "MariaDB 11 — drop-in MySQL-compatible database VM.",
        featured: false,
        icon: "🗄️",
    },
    CatalogTemplate {
        name: "redis-7",
        version: "1.0.0",
        source_disk: UBUNTU_2404_DISK,
        download_url: None,
        cloud_init: true,
        os_family: "linux",
        category: "Database",
        workload: "database",
        description: "Redis 7 cache node — small footprint, cloud-init.",
        featured: false,
        icon: "🗄️",
    },
    CatalogTemplate {
        name: "photon-os",
        version: "1.0.0",
        source_disk: UBUNTU_2404_DISK,
        download_url: None,
        cloud_init: true,
        os_family: "linux",
        category: "Appliance",
        workload: "appliance",
        description: "VMware Photon OS — minimal container host appliance.",
        featured: true,
        icon: "📦",
    },
    CatalogTemplate {
        name: "nginx-proxy",
        version: "1.0.0",
        source_disk: UBUNTU_2404_DISK,
        download_url: None,
        cloud_init: true,
        os_family: "linux",
        category: "Appliance",
        workload: "appliance",
        description: "Nginx reverse proxy — TLS termination and load balancing.",
        featured: false,
        icon: "📦",
    },
    CatalogTemplate {
        name: "wireguard-vpn",
        version: "1.0.0",
        source_disk: UBUNTU_2404_DISK,
        download_url: None,
        cloud_init: true,
        os_family: "linux",
        category: "Appliance",
        workload: "appliance",
        description: "WireGuard VPN gateway — secure remote access to the datacenter.",
        featured: false,
        icon: "📦",
    },
    CatalogTemplate {
        name: "rhel-10",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/rocky-10.qcow2",
        download_url: Some(
            "https://dl.rockylinux.org/pub/rocky/10/images/x86_64/Rocky-10-GenericCloud-Base.latest.x86_64.qcow2",
        ),
        cloud_init: true,
        os_family: "linux",
        category: "Linux",
        workload: "rhel",
        description: "RHEL 10-compatible profile (Rocky/Alma 10 golden disk path).",
        featured: false,
        icon: "🐧",
    },
    CatalogTemplate {
        name: "gpu-worker",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/gpu-worker.qcow2",
        download_url: Some(
            "https://cloud-images.ubuntu.com/releases/24.04/release/ubuntu-24.04-server-cloudimg-amd64.img",
        ),
        cloud_init: true,
        os_family: "linux",
        category: "Linux",
        workload: "gpu",
        description: "GPU compute worker — CUDA-ready Ubuntu, attach NVIDIA passthrough or vGPU.",
        featured: true,
        icon: "🎮",
    },
    CatalogTemplate {
        name: "k8s-node",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/k8s-node.qcow2",
        download_url: Some(
            "https://cloud-images.ubuntu.com/releases/24.04/release/ubuntu-24.04-server-cloudimg-amd64.img",
        ),
        cloud_init: true,
        os_family: "linux",
        category: "Linux",
        workload: "kubernetes",
        description: "Kubernetes node — containerd, kubeadm-friendly cloud-init.",
        featured: true,
        icon: "☸",
    },
    CatalogTemplate {
        name: "ai-inference-node",
        version: "1.0.0",
        source_disk: "/var/lib/libvirt/images/ai-inference-node.qcow2",
        download_url: Some(
            "https://cloud-images.ubuntu.com/releases/24.04/release/ubuntu-24.04-server-cloudimg-amd64.img",
        ),
        cloud_init: true,
        os_family: "linux",
        category: "Linux",
        workload: "ai-inference",
        description: "AI inference node — GPU + Python stack for model serving.",
        featured: true,
        icon: "🤖",
    },
];

/// Best-effort template name from natural-language VM create prompts (Spotlight / Zeus).
pub fn default_template_for_natural_language(query: &str) -> &'static str {
    let ql = query.to_lowercase();
    if ql.contains("windows") || ql.contains("win10") || ql.contains("win11") {
        if ql.contains("2022") || ql.contains("server 2022") {
            return "windows-server-2022";
        }
        if ql.contains("2025") || ql.contains("server 2025") {
            return "windows-server-2025";
        }
        if ql.contains("win10") || ql.contains("windows 10") || ql.contains("windows-10") {
            return "win10";
        }
        return "win11";
    }
    if ql.contains("26.04") || ql.contains("26-04") {
        return "ubuntu-26.04";
    }
    if ql.contains("25.10") || ql.contains("25-10") {
        return "ubuntu-25.10";
    }
    if ql.contains("rocky") || ql.contains("rhel") {
        return "rocky-10";
    }
    if ql.contains("debian") {
        return "debian-13";
    }
    if ql.contains("fedora") {
        return "fedora-44";
    }
    if ql.contains("latest") || (ql.contains("ubuntu") && ql.contains("lts")) {
        return "ubuntu-26.04";
    }
    "ubuntu-24.04"
}

const RETIRED_TEMPLATE_NAMES: &[&str] = &[
    "ubuntu-22.04",
    "debian-12",
    "centos-stream-9",
    "rocky-9",
    "alma-9",
    "fedora-40",
    "fedora-41",
    "fedora-42",
    "fedora-43",
    "rhel-9",
];

/// Remove marketplace rows that are no longer in the bundled catalog (e.g. fedora-40).
pub async fn prune_stale_marketplace_templates(pool: &DbPool) -> anyhow::Result<u64> {
    let retired_json = serde_json::to_string(RETIRED_TEMPLATE_NAMES).unwrap_or_default();
    let retired =
        crate::db::query("DELETE FROM templates WHERE name IN (SELECT value FROM json_each(?))")
            .bind(retired_json)
            .execute(pool)
            .await?
            .rows_affected();

    let catalog_json = serde_json::to_string(
        &CATALOG
            .iter()
            .map(|t| serde_json::json!({"name": t.name, "version": t.version}))
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".into());
    let result = crate::db::query(
        "DELETE FROM templates
         WHERE marketplace = TRUE
           AND NOT EXISTS (
             SELECT 1 FROM json_each(?) j
             WHERE json_extract(j.value, '$.name') = templates.name
               AND json_extract(j.value, '$.version') = templates.version
           )",
    )
    .bind(&catalog_json)
    .execute(pool)
    .await?;
    Ok(retired + result.rows_affected())
}

/// Insert bundled marketplace templates (idempotent).
pub async fn seed_default_templates(pool: &DbPool) -> anyhow::Result<usize> {
    let mut inserted = 0usize;
    // Seed the whole catalog in ONE transaction. Committing each row separately
    // meant N fsyncs at boot (one insert tripped the >1s slow-query log); a single
    // commit collapses that to one durable write.
    let mut tx = pool.begin().await?;
    for t in CATALOG {
        let fw = catalog_firewall_profile(t);
        let result = crate::db::query(
            "INSERT INTO templates (id, name, version, source_disk, cloud_init, os_family, category, workload, description, featured, marketplace, icon, firewall_profile, approval_status)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, TRUE, ?, ?, 'approved')
             ON CONFLICT (name, version) DO UPDATE SET
               firewall_profile = EXCLUDED.firewall_profile,
               workload = EXCLUDED.workload,
               source_disk = EXCLUDED.source_disk,
               description = EXCLUDED.description,
               featured = EXCLUDED.featured,
               icon = EXCLUDED.icon,
               cloud_init = EXCLUDED.cloud_init,
               os_family = EXCLUDED.os_family,
               category = EXCLUDED.category",
        )
        .bind(Uuid::new_v4())
        .bind(t.name)
        .bind(t.version)
        .bind(t.source_disk)
        .bind(t.cloud_init)
        .bind(t.os_family)
        .bind(t.category)
        .bind(t.workload)
        .bind(t.description)
        .bind(t.featured)
        .bind(t.icon)
        .bind(fw)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() > 0 {
            inserted += 1;
        }
    }
    tx.commit().await?;
    let _ = prune_stale_marketplace_templates(pool).await?;
    Ok(inserted)
}

/// Public download URL for a bundled template (used for auto-fetch on first deploy).
pub fn download_url_for(name: &str, version: &str) -> Option<&'static str> {
    CATALOG
        .iter()
        .find(|t| t.name == name && t.version == version)
        .and_then(|t| t.download_url)
}

pub fn download_url_for_ref(template_ref: &str) -> Option<&'static str> {
    let (name, version) = template_ref
        .split_once('@')
        .unwrap_or((template_ref, "1.0.0"));
    download_url_for(name, version)
}

pub async fn ensure_default_templates(pool: &DbPool) -> anyhow::Result<()> {
    seed_default_templates(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::default_template_for_natural_language;

    #[test]
    fn nl_os_windows_server_and_desktop() {
        assert_eq!(
            default_template_for_natural_language("create windows server 2025 vm"),
            "windows-server-2025"
        );
        assert_eq!(
            default_template_for_natural_language("create windows server 2022"),
            "windows-server-2022"
        );
        assert_eq!(
            default_template_for_natural_language("create windows 10 desktop"),
            "win10"
        );
        assert_eq!(
            default_template_for_natural_language("create windows 11 vm"),
            "win11"
        );
    }

    #[test]
    fn nl_os_ubuntu_latest_lts() {
        assert_eq!(
            default_template_for_natural_language("create latest ubuntu lts vm"),
            "ubuntu-26.04"
        );
    }
}
