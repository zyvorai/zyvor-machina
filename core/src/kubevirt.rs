// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Generate KubeVirt + CDI manifests to migrate a libvirt qcow2 (or raw) root disk to Kubernetes,
//! following the same layering as [hyper2kvm](https://github.com/ssahani/hyper2kvm) docs: DataVolume
//! for the root image, `VirtualMachine` with virtio disks, guest-OS profiles (Linux SSH / Windows RDP),
//! and optional **virtio-win CDROM** via `containerDisk`.

use crate::config::KubeVirtConfig;
use crate::state::{DiskInfo, VmDetails};
use crate::LibvirtError;
use std::path::Path;

/// Guest OS family for KubeVirt domain spec (hyper2kvm-style profiles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestOsFamily {
    Linux,
    Windows,
}

impl GuestOsFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            GuestOsFamily::Linux => "linux",
            GuestOsFamily::Windows => "windows",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "linux" | "lin" => Some(GuestOsFamily::Linux),
            "windows" | "win" | "mswin" => Some(GuestOsFamily::Windows),
            "auto" => None,
            _ => None,
        }
    }
}

/// Resolve guest OS from explicit hint, VM name, disk path, or libvirt `os_type`.
pub fn resolve_guest_os(
    explicit: Option<&str>,
    vm_or_disk_name: &str,
    disk_path: &str,
    os_type: Option<&str>,
) -> GuestOsFamily {
    if let Some(s) = explicit {
        if let Some(f) = GuestOsFamily::parse(s) {
            return f;
        }
    }
    if let Some(ot) = os_type {
        let l = ot.to_lowercase();
        if l.contains("win") {
            return GuestOsFamily::Windows;
        }
    }
    let probe = format!(
        "{} {}",
        vm_or_disk_name.to_lowercase(),
        disk_path.to_lowercase()
    );
    if looks_windows_name(&probe) {
        GuestOsFamily::Windows
    } else {
        GuestOsFamily::Linux
    }
}

fn looks_windows_name(s: &str) -> bool {
    const WIN: &[&str] = &[
        "windows",
        "win-",
        "win_",
        "win2k",
        "w2k",
        "ws20",
        "ws19",
        "ws16",
        "win10",
        "win11",
        "winserver",
        "hyper-v",
    ];
    WIN.iter().any(|w| s.contains(w))
}

/// API / UI payload: multi-doc YAML plus copy-paste hints.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KubeVirtBundle {
    pub libvirt_vm: String,
    /// Host path to the root disk image (libvirt XML or standalone qcow2 upload).
    pub libvirt_root_disk: String,
    /// `linux` or `windows` — domain features / ports match hyper2kvm guest profiles.
    pub guest_os: String,
    pub namespace: String,
    pub virtual_machine_name: String,
    pub datavolume_name: String,
    /// PVC/DataVolume upload size (Gi) used in manifests and virtctl.
    pub upload_size_gi: u32,
    /// When true, daemon may run `kubectl` / `virtctl` for this VM (`[kubevirt] exec_enabled`).
    pub cluster_exec_enabled: bool,
    /// CDI DataVolume + KubeVirt VirtualMachine, `---` separated.
    pub yaml: String,
    /// Example `virtctl image-upload` for the disk on the hypervisor.
    pub virtctl_image_upload_example: String,
}

fn is_file_disk(d: &DiskInfo) -> bool {
    d.device != "cdrom"
        && d.device != "floppy"
        && !d.source.is_empty()
        && d.source != crate::UNKNOWN
}

/// First file-backed boot disk on a libvirt guest.
pub fn pick_root_boot_disk(details: &VmDetails) -> Result<&DiskInfo, LibvirtError> {
    let pathish = |s: &str| {
        s.ends_with(".qcow2") || s.ends_with(".QCOW2") || s.ends_with(".raw") || s.ends_with(".img")
    };
    for d in &details.disks {
        if is_file_disk(d) && pathish(&d.source) {
            return Ok(d);
        }
    }
    for d in &details.disks {
        if is_file_disk(d) {
            return Ok(d);
        }
    }
    Err(LibvirtError::Invalid(
        "No file-backed root disk found for KubeVirt export (need a disk volume with a file path)."
            .into(),
    ))
}

/// RFC 1123-ish label: lowercase, alphanumeric + hyphen, max 63, non-empty.
pub fn sanitize_k8s_label(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::new();
    for c in lower.chars() {
        let c = if c.is_ascii_alphanumeric() { c } else { '-' };
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        return "vm".to_string();
    }
    if out.len() <= 63 {
        return out;
    }
    out.chars()
        .take(63)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn storage_gi_for_disk(path: &str, memory_mb: u64, padding_gi: u32) -> u32 {
    let from_file = std::fs::metadata(path).ok().map(|m| {
        let b = m.len();
        b.div_ceil(1 << 30) as u32
    });
    let fallback = memory_mb.div_ceil(1024).max(1) as u32;
    from_file
        .unwrap_or(fallback)
        .saturating_add(padding_gi)
        .max(1)
}

fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn default_include_virtio_cdrom(guest: GuestOsFamily, explicit: bool) -> bool {
    if explicit {
        return true;
    }
    matches!(guest, GuestOsFamily::Windows)
}

struct BundleBuildInput<'a> {
    source_label: &'a str,
    root_path: &'a str,
    cfg: &'a KubeVirtConfig,
    namespace: &'a str,
    vm_k8s: &'a str,
    dv_name: &'a str,
    storage_gi: u32,
    storage_class_line: &'a str,
    cores: u32,
    mem_gi: u64,
    guest: GuestOsFamily,
    include_virtio_cdrom: bool,
}

fn build_bundle(input: BundleBuildInput<'_>) -> Result<KubeVirtBundle, LibvirtError> {
    let virtio_img = input.cfg.virtio_container_disk_image.trim();
    if input.include_virtio_cdrom && virtio_img.is_empty() {
        return Err(LibvirtError::Internal(
            "kubevirt.virtio_container_disk_image is empty".into(),
        ));
    }

    let machine = input.cfg.machine_type.trim();
    let machine_line = if machine.is_empty() {
        "q35".to_string()
    } else {
        machine.to_string()
    };

    let guest_label = input.guest.as_str();
    let ns = input.namespace;
    let dv_name = input.dv_name;
    let vm_k8s = input.vm_k8s;
    let storage_gi = input.storage_gi;
    let root_path = input.root_path;
    let source_label = input.source_label;

    let mut dv = String::new();
    dv.push_str("# CDI upload DataVolume — import qcow2/raw into the cluster PVC.\n");
    dv.push_str("# 1) virtctl image-upload (see virtctl_image_upload_example in API JSON)\n");
    dv.push_str("# 2) kubectl apply -f this file (or paste both documents)\n");
    dv.push_str("# 3) virtctl start <vm> -n namespace\n");
    dv.push_str("apiVersion: cdi.kubevirt.io/v1beta1\nkind: DataVolume\nmetadata:\n");
    dv.push_str(&format!("  name: {dv_name}\n  namespace: {ns}\n"));
    dv.push_str("  labels:\n    machina.io/guest-os: \"");
    dv.push_str(guest_label);
    dv.push_str("\"\n    machina.io/source: \"");
    dv.push_str(&yaml_escape(source_label));
    dv.push_str("\"\nspec:\n  source:\n    upload: {}\n  pvc:\n    accessModes:\n      - ReadWriteOnce\n    resources:\n      requests:\n");
    dv.push_str(&format!("        storage: {storage_gi}Gi\n"));
    dv.push_str(input.storage_class_line);

    let mut vm = String::new();
    vm.push_str("# KubeVirt VM — virtio root + guest-OS profile (hyper2kvm-style).\n");
    if input.include_virtio_cdrom {
        vm.push_str("# virtio-win CD: attach drivers after migration (containerDisk).\n");
    }
    vm.push_str("apiVersion: kubevirt.io/v1\nkind: VirtualMachine\nmetadata:\n");
    vm.push_str(&format!("  name: {vm_k8s}\n  namespace: {ns}\n"));
    vm.push_str("  labels:\n    machina.io/guest-os: \"");
    vm.push_str(guest_label);
    vm.push_str("\"\n    machina.io/source: \"");
    vm.push_str(&yaml_escape(source_label));
    vm.push_str("\"\n  annotations:\n    machina.io/guest-os: \"");
    vm.push_str(guest_label);
    vm.push_str("\"\nspec:\n  runStrategy: Halted\n  template:\n    metadata:\n      labels:\n");
    vm.push_str(&format!("        kubevirt.io/vm: {vm_k8s}\n"));
    vm.push_str("        machina.io/guest-os: \"");
    vm.push_str(guest_label);
    vm.push_str("\"\n    spec:\n      domain:\n        machine:\n");
    vm.push_str(&format!("          type: {machine_line}\n"));
    if matches!(input.guest, GuestOsFamily::Windows) {
        // dockur / Win10–11 goldens boot UEFI; SeaBIOS would not start the qcow2.
        vm.push_str("        firmware:\n          bootloader:\n            efi:\n              secureBoot: false\n");
    }
    vm.push_str("        cpu:\n");
    vm.push_str(&format!("          cores: {}\n", input.cores.max(1)));
    vm.push_str("        devices:\n          disks:\n            - name: rootdisk\n              disk:\n                bus: virtio\n");
    if input.include_virtio_cdrom {
        vm.push_str(
            "            - name: virtiocd\n              cdrom:\n                bus: sata\n",
        );
    }
    if matches!(input.guest, GuestOsFamily::Windows) {
        // Win11 (and dockur win10/win11) expect a TPM; harmless on Win10.
        vm.push_str("          tpm:\n            model: tpm-tis\n            interface: tpm-crb\n");
    }
    vm.push_str("          interfaces:\n            - name: default\n              masquerade: {}\n              model: virtio\n");
    if matches!(input.guest, GuestOsFamily::Windows) {
        vm.push_str("              ports:\n                - name: rdp\n                  port: 3389\n                  protocol: TCP\n");
    } else {
        vm.push_str("              ports:\n                - name: ssh\n                  port: 22\n                  protocol: TCP\n");
        vm.push_str("          rng: {}\n");
    }
    if matches!(input.guest, GuestOsFamily::Windows) {
        vm.push_str("        features:\n          acpi: {}\n          apic: {}\n          smm: {}\n          hyperv:\n            relaxed: {}\n            vapic: {}\n            spinlocks:\n              spinlocks: 8191\n");
        vm.push_str("        clock:\n          utc: {}\n          timer:\n            hpet:\n              present: false\n            pit:\n              tickPolicy: delay\n            rtc:\n              tickPolicy: catchup\n            hyperv: {}\n");
    } else {
        vm.push_str("        features:\n          acpi: {}\n          apic: {}\n");
        vm.push_str("        clock:\n          utc: {}\n          timer:\n            hpet:\n              present: false\n            pit:\n              tickPolicy: delay\n            rtc:\n              tickPolicy: catchup\n");
    }
    vm.push_str("        resources:\n          requests:\n");
    vm.push_str(&format!("            memory: {}Gi\n", input.mem_gi.max(1)));
    vm.push_str("      networks:\n        - name: default\n          pod: {}\n      volumes:\n        - name: rootdisk\n          dataVolume:\n");
    vm.push_str(&format!("            name: {dv_name}\n"));
    if input.include_virtio_cdrom {
        vm.push_str("        - name: virtiocd\n          containerDisk:\n            image: \"");
        vm.push_str(&yaml_escape(virtio_img));
        vm.push_str("\"\n            imagePullPolicy: IfNotPresent\n");
    }

    let yaml = format!("{}\n---\n{}", dv.trim_end(), vm.trim_end());

    let timeout_m = input.cfg.upload_timeout_minutes.max(1);
    let virtctl_image_upload_example = format!(
        "virtctl image-upload dv {dv_name} --size={storage_gi}Gi --image-path='{root_path}' -n {ns} --insecure --upload-image-timeout={timeout_m}m",
    );

    Ok(KubeVirtBundle {
        libvirt_vm: source_label.to_string(),
        libvirt_root_disk: root_path.to_string(),
        guest_os: guest_label.to_string(),
        namespace: ns.to_string(),
        virtual_machine_name: vm_k8s.to_string(),
        datavolume_name: dv_name.to_string(),
        upload_size_gi: storage_gi,
        cluster_exec_enabled: input.cfg.exec_enabled,
        yaml,
        virtctl_image_upload_example,
    })
}

fn storage_class_line(cfg: &KubeVirtConfig, storage_class_override: Option<&str>) -> String {
    let storage_class_effective = storage_class_override
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            let d = cfg.default_storage_class.trim();
            if d.is_empty() {
                None
            } else {
                Some(d.to_string())
            }
        });
    storage_class_effective
        .map(|sc| format!("    storageClassName: \"{}\"\n", yaml_escape(&sc)))
        .unwrap_or_default()
}

/// Build CDI DataVolume (upload) + KubeVirt VM from an on-disk qcow2 (no libvirt domain required).
#[allow(clippy::too_many_arguments)]
pub fn kubevirt_bundle_from_qcow2(
    qcow2_path: &str,
    cfg: &KubeVirtConfig,
    guest_os: Option<&str>,
    namespace_override: Option<&str>,
    k8s_name_override: Option<&str>,
    datavolume_name_override: Option<&str>,
    storage_gi_override: Option<u32>,
    storage_class_override: Option<&str>,
    vcpus_override: Option<u32>,
    memory_mb_override: Option<u64>,
    include_virtio_cdrom: Option<bool>,
) -> Result<KubeVirtBundle, LibvirtError> {
    let path = qcow2_path.trim();
    if !path.starts_with('/') {
        return Err(LibvirtError::Invalid(format!(
            "qcow2_path must be an absolute path on the hypervisor, got: {path}"
        )));
    }
    if path.contains("..") {
        return Err(LibvirtError::Invalid("path traversal not allowed".into()));
    }
    let lower = path.to_lowercase();
    if !(lower.ends_with(".qcow2") || lower.ends_with(".raw") || lower.ends_with(".img")) {
        return Err(LibvirtError::Invalid(format!(
            "KubeVirt disk upload expects .qcow2, .raw, or .img — got: {path}"
        )));
    }
    if !Path::new(path).is_file() {
        return Err(LibvirtError::Invalid(format!(
            "Disk image not found on hypervisor: {path}"
        )));
    }

    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("disk");
    let guest = resolve_guest_os(guest_os, stem, path, None);
    let virtio_cd =
        include_virtio_cdrom.unwrap_or_else(|| default_include_virtio_cdrom(guest, false));

    // namespace_override is caller/query-string controlled and, unlike the name
    // fields below, was interpolated into the generated YAML unsanitized — a
    // namespace containing a newline could inject arbitrary keys/documents into
    // the manifest a caller later `kubectl apply`s. Route it through the same
    // RFC-1123-label sanitizer used for names.
    let ns = sanitize_k8s_label(
        namespace_override
            .filter(|s| !s.is_empty())
            .unwrap_or(cfg.default_namespace.as_str()),
    );
    let vm_k8s = sanitize_k8s_label(k8s_name_override.filter(|s| !s.is_empty()).unwrap_or(stem));
    let dv_name = sanitize_k8s_label(
        datavolume_name_override
            .filter(|s| !s.is_empty())
            .unwrap_or(&format!("{vm_k8s}-root")),
    );

    let memory_mb = memory_mb_override.unwrap_or(match guest {
        GuestOsFamily::Windows => 8192,
        GuestOsFamily::Linux => 4096,
    });
    let storage_gi = storage_gi_override
        .unwrap_or_else(|| storage_gi_for_disk(path, memory_mb, cfg.datavolume_padding_gi));
    let cores = vcpus_override
        .unwrap_or(match guest {
            GuestOsFamily::Windows => 4,
            GuestOsFamily::Linux => 2,
        })
        .max(1);
    let mem_gi = memory_mb.div_ceil(1024).max(1);
    let sc_line = storage_class_line(cfg, storage_class_override);

    build_bundle(BundleBuildInput {
        source_label: stem,
        root_path: path,
        cfg,
        namespace: &ns,
        vm_k8s: &vm_k8s,
        dv_name: &dv_name,
        storage_gi,
        storage_class_line: &sc_line,
        cores,
        mem_gi,
        guest,
        include_virtio_cdrom: virtio_cd,
    })
}

/// Build CDI DataVolume (upload) + KubeVirt VM with virtio root + optional virtio-win CDROM.
#[allow(clippy::too_many_arguments)]
pub fn kubevirt_bundle_from_libvirt_vm(
    details: &VmDetails,
    libvirt_name: &str,
    cfg: &KubeVirtConfig,
    namespace_override: Option<&str>,
    k8s_name_override: Option<&str>,
    datavolume_name_override: Option<&str>,
    storage_gi_override: Option<u32>,
    storage_class_override: Option<&str>,
    include_virtio_cdrom: bool,
) -> Result<KubeVirtBundle, LibvirtError> {
    let disk = pick_root_boot_disk(details)?;
    let root_path = disk.source.trim();
    if !root_path.starts_with('/') {
        return Err(LibvirtError::Invalid(format!(
            "KubeVirt export needs an absolute disk path on the hypervisor, got: {root_path}"
        )));
    }

    let guest = resolve_guest_os(None, libvirt_name, root_path, Some(&details.os_type));
    let virtio_cd = include_virtio_cdrom;

    // See the matching comment in kubevirt_bundle_from_qcow2: this value is
    // caller-controlled and gets embedded directly in generated YAML, so it
    // needs the same sanitization as the name fields below.
    let ns = sanitize_k8s_label(
        namespace_override
            .filter(|s| !s.is_empty())
            .unwrap_or(cfg.default_namespace.as_str()),
    );
    let vm_k8s = sanitize_k8s_label(
        k8s_name_override
            .filter(|s| !s.is_empty())
            .unwrap_or(libvirt_name),
    );
    let dv_name = sanitize_k8s_label(
        datavolume_name_override
            .filter(|s| !s.is_empty())
            .unwrap_or(&format!("{vm_k8s}-root")),
    );

    let storage_gi = storage_gi_override.unwrap_or_else(|| {
        storage_gi_for_disk(root_path, details.memory_mb, cfg.datavolume_padding_gi)
    });
    let sc_line = storage_class_line(cfg, storage_class_override);
    let cores = details.vcpus.max(1);
    let mem_gi = details.memory_mb.div_ceil(1024).max(1);

    build_bundle(BundleBuildInput {
        source_label: libvirt_name,
        root_path,
        cfg,
        namespace: &ns,
        vm_k8s: &vm_k8s,
        dv_name: &dv_name,
        storage_gi,
        storage_class_line: &sc_line,
        cores,
        mem_gi,
        guest,
        include_virtio_cdrom: virtio_cd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_k8s_label_truncates() {
        let long = "a".repeat(80);
        let s = sanitize_k8s_label(&long);
        assert!(s.len() <= 63);
    }

    #[test]
    fn detect_windows_from_name() {
        assert_eq!(
            resolve_guest_os(None, "win2k22", "/var/lib/images/disk.qcow2", None),
            GuestOsFamily::Windows
        );
        assert_eq!(
            resolve_guest_os(Some("linux"), "win2k22", "/x/disk.qcow2", None),
            GuestOsFamily::Linux
        );
    }

    #[test]
    fn windows_bundle_includes_uefi_and_tpm() {
        let cfg = KubeVirtConfig::default();
        // Create a tiny temp file so path checks pass.
        let dir = std::env::temp_dir().join("machina-kubevirt-win-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("win11.qcow2");
        std::fs::write(&path, b"fake").unwrap();
        let bundle = kubevirt_bundle_from_qcow2(
            path.to_str().unwrap(),
            &cfg,
            Some("windows"),
            None,
            None,
            None,
            Some(64),
            None,
            Some(4),
            Some(8192),
            Some(false),
        )
        .unwrap();
        assert!(bundle.yaml.contains("efi:"));
        assert!(bundle.yaml.contains("secureBoot: false"));
        assert!(bundle.yaml.contains("tpm:"));
        assert!(bundle.yaml.contains("port: 3389"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pick_disk_prefers_qcow2() {
        let d = VmDetails {
            name: "t".into(),
            uuid: "u".into(),
            state: "shutoff".into(),
            vcpus: 1,
            memory_mb: 1024,
            os_type: "hvm".into(),
            arch: "x86_64".into(),
            autostart: false,
            persistent: true,
            interfaces: vec![],
            disks: vec![
                DiskInfo {
                    device: "cdrom".into(),
                    source: "/var/lib/libvirt/images/seed.iso".into(),
                    driver: "raw".into(),
                    target: "sda".into(),
                    bus: String::new(),
                    cache: String::new(),
                    readonly: false,
                    shareable: false,
                    capacity_bytes: None,
                    allocation_bytes: None,
                    physical_bytes: None,
                },
                DiskInfo {
                    device: "disk".into(),
                    source: "/var/lib/libvirt/images/guest.qcow2".into(),
                    driver: "qcow2".into(),
                    target: "vda".into(),
                    bus: String::new(),
                    cache: String::new(),
                    readonly: false,
                    shareable: false,
                    capacity_bytes: None,
                    allocation_bytes: None,
                    physical_bytes: None,
                },
            ],
            filesystems: vec![],
            libvirt_connection: None,
            guest_ip: None,
            backend: None,
            fluxvm_backend: None,
        };
        let disk = pick_root_boot_disk(&d).unwrap();
        assert!(disk.source.ends_with(".qcow2"));
    }
}
