// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

// ── VM Types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmInfo {
    pub name: String,
    pub state: String,
    pub vcpus: u32,
    pub memory_mb: u64,
    /// `system` / `session` when `[libvirt] dual_connection` is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libvirt_connection: Option<String>,
    /// Best-effort guest IPv4 (list view; running VMs only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guest_ip: Option<String>,
    /// Every guest address seen (all NICs, IPv4 then IPv6).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guest_ips: Vec<String>,
    /// `fluxvm` for VMs managed by `fluxvm-api`; absent for libvirt domains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// FluxVM hypervisor (`qemu`, `cloud-hypervisor`, `firecracker`, `flux-vm`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fluxvm_backend: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmDetails {
    pub name: String,
    pub uuid: String,
    pub state: String,
    pub vcpus: u32,
    pub memory_mb: u64,
    pub os_type: String,
    pub arch: String,
    pub autostart: bool,
    pub persistent: bool,
    pub interfaces: Vec<InterfaceInfo>,
    pub disks: Vec<DiskInfo>,
    #[serde(default)]
    pub filesystems: Vec<FilesystemInfo>,
    /// `system` / `session` when bound to a specific libvirt scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libvirt_connection: Option<String>,
    /// Best-effort guest IPv4 from DHCP lease, ARP, or qemu-guest-agent (when running).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guest_ip: Option<String>,
    /// `fluxvm` for VMs managed by `fluxvm-api`; absent for libvirt domains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// FluxVM hypervisor (`qemu`, `cloud-hypervisor`, `firecracker`, `flux-vm`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fluxvm_backend: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceInfo {
    pub mac_address: String,
    pub source: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub device: String,
    pub source: String,
    pub driver: String,
    pub target: String,
    #[serde(default)]
    pub bus: String,
    #[serde(default)]
    pub cache: String,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub shareable: bool,
    /// Guest-visible capacity when libvirt exposes block info (bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity_bytes: Option<u64>,
    /// Host allocation for the backing store (bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocation_bytes: Option<u64>,
    /// Physical size on host (bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemInfo {
    /// Host directory path for `type='mount'` shares.
    pub source: String,
    /// Guest mount tag (libvirt calls this `target dir`, but it's not a guest path).
    pub mount_tag: String,
    /// `virtiofs` etc.
    #[serde(default)]
    pub driver: String,
    /// `passthrough` etc.
    #[serde(default)]
    pub accessmode: String,
    /// Whether virtiofs xattr support is enabled (requires libvirt virtiofsd wiring).
    #[serde(default)]
    pub xattr: bool,
}

// ── Snapshot Types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub name: String,
    pub vm_name: String,
    pub creation_time: i64,
    pub state: String,
    pub description: String,
    pub parent: String,
    pub is_current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSnapshotRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub disk_only: bool,
    /// `auto` (default), `external`, or `internal`.
    #[serde(default)]
    pub storage_mode: String,
    /// `internal` or `external` (ignored when disk_only=true).
    #[serde(default)]
    pub memory_snapshot: String,
    /// Absolute path for external memory snapshot file (optional; server may auto-generate).
    #[serde(default)]
    pub memory_file: String,
    /// When set, generate external disk snapshot files under this directory.
    #[serde(default)]
    pub external_disk_dir: String,
    /// When set, generate external memory snapshot file under this directory.
    #[serde(default)]
    pub external_memory_dir: String,
    /// Per-disk snapshot overrides (by disk target, e.g. vda).
    #[serde(default)]
    pub disks: Vec<SnapshotDiskSpec>,
    /// Use libvirt atomic snapshot flag (recommended).
    #[serde(default = "default_true")]
    pub atomic: bool,
    /// Allow reuse of pre-created external snapshot files.
    #[serde(default)]
    pub reuse_external: bool,
    /// QEMU guest agent filesystem freeze before snapshot (app-consistent).
    #[serde(default)]
    pub quiesce: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDiskSpec {
    /// Disk identifier: target dev name like `vda` (recommended).
    pub name: String,
    /// `no`, `external`, `internal`, or `manual` (see libvirt formatsnapshot).
    #[serde(default)]
    pub snapshot: String,
    /// Optional output file for `snapshot=external` (absolute path). When empty, libvirt may auto-generate.
    #[serde(default)]
    pub file: String,
    /// Optional driver type for the external file (e.g. `qcow2`).
    #[serde(default)]
    pub driver: String,
}

// ── Network Types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub name: String,
    pub uuid: String,
    pub active: bool,
    pub persistent: bool,
    pub autostart: bool,
    pub bridge: String,
}

// ── Storage Types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoragePoolInfo {
    pub name: String,
    pub uuid: String,
    pub state: String,
    pub capacity_gb: f64,
    pub allocation_gb: f64,
    pub available_gb: f64,
    pub autostart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageVolumeInfo {
    pub name: String,
    pub pool: String,
    pub capacity_gb: f64,
    pub allocation_gb: f64,
    pub path: String,
    pub vol_type: String,
}

// -- Backup Types -------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub id: String,
    pub timestamp: String,
    pub vm_filter: String,
    pub vm_count: u32,
    pub net_count: u32,
    pub with_disks: bool,
    pub nfs_target: String,
    pub size: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackupRequest {
    #[serde(default)]
    pub vm_name: String,
    #[serde(default)]
    pub with_disks: bool,
    #[serde(default)]
    pub nfs_target: String,
    #[serde(default)]
    pub retain: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreRequest {
    pub backup_id: String,
}

// ── Node / Host Types ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub hostname: String,
    pub hypervisor: String,
    pub hypervisor_version: String,
    pub lib_version: String,
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub cpu_threads: u32,
    pub cpu_sockets: u32,
    pub memory_mb: u64,
    pub numa_nodes: u32,
    pub active_vms: u32,
    pub defined_vms: u32,
}

// ── Metrics Types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VmBlockDeviceMetrics {
    /// libvirt block target dev (e.g. vda, sda).
    pub device: String,
    pub rd_bytes: u64,
    pub wr_bytes: u64,
    pub rd_ops: u64,
    pub wr_ops: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VmVcpuMetrics {
    pub vcpu: u32,
    pub cpu_time_ns: u64,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VmNetDeviceMetrics {
    /// libvirt interface target dev (e.g. vnet0).
    pub device: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmMetrics {
    pub name: String,
    /// libvirt state: running, shutoff, paused, etc.
    pub state: String,
    pub running: bool,
    pub cpu_time_ns: u64,
    pub vcpus: u32,
    pub memory_total_mb: u64,
    pub memory_used_mb: u64,
    pub memory_pct: f64,
    pub disk_rd_bytes: u64,
    pub disk_wr_bytes: u64,
    pub disk_rd_ops: u64,
    pub disk_wr_ops: u64,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vcpus_detail: Vec<VmVcpuMetrics>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disks: Vec<VmBlockDeviceMetrics>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nets: Vec<VmNetDeviceMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cgroup: Option<crate::host_linux_obs::VmCgroupStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libvirt_connection: Option<String>,
}

// ── Audit / Event Types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp: String,
    pub action: String,
    pub target: String,
    pub result: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub actor: String,
}

// ── Clone Request ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneVmRequest {
    pub new_name: String,
    /// `linked` (default), `full`, or `xml` (legacy shared disk — not recommended).
    #[serde(default)]
    pub clone_mode: String,
}

// ── Create VM Request ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVmRequest {
    pub name: String,
    #[serde(default = "default_vcpus")]
    pub vcpus: u32,
    #[serde(default = "default_memory")]
    pub memory_mb: u64,
    #[serde(default = "default_disk_gb")]
    pub disk_gb: u64,
    #[serde(default)]
    pub iso: String,
    #[serde(default = "default_network")]
    pub network: String,
    #[serde(default = "default_os_variant")]
    pub os_variant: String,
    /// Guest tuning profile: `""`/`auto`, `linux`, or `windows`.
    /// Used to pick better defaults for Windows (SPICE/QXL, virtio disk+NIC, virtio-win driver ISO).
    #[serde(default)]
    pub guest_profile: String,
    /// Optional virtio-win driver ISO (additional CD-ROM) for Windows installs.
    #[serde(default)]
    pub virtio_win_iso: String,
    /// Use an existing disk image instead of creating a new one
    #[serde(default)]
    pub existing_disk: String,
    /// Firmware type: "bios" (default) or "uefi"
    #[serde(default = "default_firmware")]
    pub firmware: String,
    /// Enforce UEFI Secure Boot with Microsoft/distribution keys enrolled (needs `firmware: "uefi"`).
    #[serde(default)]
    pub secure_boot: bool,
    /// Add an emulated TPM 2.0 (swtpm). Windows 11 needs this and `secure_boot`.
    #[serde(default)]
    pub vtpm: bool,
    /// Libvirt VNC `listen=` address (IP only). Default `127.0.0.1`; use `0.0.0.0` to match hyper2kvm-style remote display (still reach via machina's WS proxy / TLS front).
    #[serde(default = "default_graphics_listen")]
    pub graphics_listen: String,
    /// `vnc` (default, noVNC) or `spice` (spice-html5 + WS proxy).
    #[serde(default = "default_graphics_type")]
    pub graphics_type: String,
    /// Optional cloud-init / seed ISO (second CD-ROM, hyper2kvm-style). Install/boot ISO stays in `iso`.
    #[serde(default)]
    pub cloud_init_iso: String,
    /// If set (and `cloud_init_iso` is empty), machina generates a NoCloud seed ISO and attaches it as the second CD-ROM.
    #[serde(default)]
    pub cloud_init_user: String,
    /// Plaintext password to pass via cloud-init `chpasswd`. Avoid for long-lived secrets.
    #[serde(default)]
    pub cloud_init_password: String,
    /// Single-line OpenSSH public key to inject for the cloud-init user.
    #[serde(default)]
    pub cloud_init_ssh_pubkey: String,
    /// Saved template name under `/var/lib/machina/templates/{name}.json` (server applies sizing + optional `base_image`).
    #[serde(default)]
    pub saved_template: String,
    /// When using a template with `base_image`: `backing` (default) or `copy`.
    #[serde(default = "default_template_disk_mode")]
    pub template_disk_mode: String,
    /// Per-request create engine: `""` (use server default), `libvirt_xml`, or `virt_install`.
    #[serde(default)]
    pub create_backend: String,
    /// If set (and no golden/existing root disk), run `virt-builder` to build the root qcow2 from the libguestfs index (e.g. `ubuntu-22.04`).
    #[serde(default)]
    pub virt_builder_os: String,
    /// Guest hostname for `virt-builder` (optional; defaults from VM name with `_` → `-`).
    #[serde(default)]
    pub virt_builder_hostname: String,
    /// Single-line OpenSSH public key for `--ssh-inject root:file:…` (optional if root password file or server default pubkey path is set).
    #[serde(default)]
    pub virt_builder_ssh_pubkey: String,
    /// Host-local file with guest root password (single line); passed as `--root-password file:…` (never on argv).
    #[serde(default)]
    pub virt_builder_root_password_file: String,
    /// Extra packages for `virt-builder --install` (each string is one package name).
    #[serde(default)]
    pub virt_builder_packages: Vec<String>,
    /// `virt-builder --firstboot-command` entries (guest runs at first boot).
    #[serde(default)]
    pub virt_builder_firstboot_commands: Vec<String>,
    /// Pass `--selinux-relabel` to `virt-builder`.
    #[serde(default)]
    pub virt_builder_selinux_relabel: bool,
    /// After build: `virt-customize --install` for each entry.
    #[serde(default)]
    pub virt_builder_post_customize_install: Vec<String>,
    /// After build: `virt-customize --run-command` for each entry (guest must be off).
    #[serde(default)]
    pub virt_builder_post_customize_run: Vec<String>,
    /// After customize: run `virt-sysprep -a` (seal for cloning; see libguestfs virt-sysprep).
    #[serde(default)]
    pub virt_builder_sysprep: bool,
    /// Absolute path to a directory containing `mkosi.conf`; run `mkosi build` there and use the newest `.raw`/`.qcow2` artifact as the VM root disk. Mutually exclusive with `virt_builder_os`.
    #[serde(default)]
    pub mkosi_workspace: String,
    /// For multi-image mkosi workspaces (image trees): pass `--image <name>` to select a specific image.
    #[serde(default)]
    pub mkosi_image: String,
    // ── virt-install extensions (Cockpit-machines-style; require `create_backend` / default `virt_install`) ──
    /// Run `virt-install --print-xml=1` and define the domain without install media (halted shell; install later).
    #[serde(default)]
    pub virt_install_define_only: bool,
    /// Network install tree: `virt-install --location <url-or-path>` (kickstart / OS tree).
    #[serde(default)]
    pub virt_install_location: String,
    /// `virt-install --pxe --network network=<virt_install_pxe_network>`.
    #[serde(default)]
    pub virt_install_pxe: bool,
    /// Libvirt network name for PXE (defaults to `network` when empty).
    #[serde(default)]
    pub virt_install_pxe_network: String,
    /// `virt-install --install os=<libosinfo-id>` (boxed / downloaded media).
    #[serde(default)]
    pub virt_install_install_os: String,
    /// `virt-install --extra-args` (kernel / installer args; pairs with `--location` or `--cdrom`).
    #[serde(default)]
    pub virt_install_extra_args: String,
    /// Root disk from a storage pool volume: `virt-install --disk vol=<pool>/<name>,bus=virtio`.
    #[serde(default)]
    pub root_disk_storage_pool: String,
    #[serde(default)]
    pub root_disk_storage_volume: String,
    /// `virt-install --check path_in_use=off` (busy images / pool volumes).
    #[serde(default)]
    pub virt_install_path_in_use_check_off: bool,
    /// New overlay disk with `backing_store=` (cloud / golden image on host); implies `--import`.
    #[serde(default)]
    pub virt_install_disk_backing_store: String,
    /// Pass `virt-install --unattended` (Cockpit-style automated install).
    #[serde(default)]
    pub virt_install_unattended: bool,
    /// `virt-install --unattended admin-password=…`
    #[serde(default)]
    pub virt_install_admin_password: String,
    /// `virt-install --unattended user-login=…`
    #[serde(default)]
    pub virt_install_user_login: String,
    /// `virt-install --unattended user-password=…`
    #[serde(default)]
    pub virt_install_user_password: String,
    /// When `[libvirt] dual_connection`: `system` or `session` — which libvirt URI defines this guest.
    #[serde(default)]
    pub libvirt_connection: String,
    /// VM backend: `""`/`libvirt` (default) or `fluxvm` (needs `[fluxvm] enabled`).
    #[serde(default)]
    pub backend: String,
    /// FluxVM hypervisor: `""` (use `[fluxvm] default_backend`), `auto`, `qemu`,
    /// `cloud-hypervisor`, `firecracker`, `flux-vm`.
    #[serde(default)]
    pub fluxvm_backend: String,
    /// FluxVM base image path on the host (cloned per VM). Falls back to `existing_disk`.
    #[serde(default)]
    pub fluxvm_image: String,
    /// FluxVM host bridge for a TAP NIC (FluxVM attaches its eBPF VM edge to the tap).
    #[serde(default)]
    pub fluxvm_bridge: String,
    /// FluxVM network when neither `fluxvm_bridge` nor `fluxvm_direct_uplink` is set:
    /// `""`/`netns` (TAP in its own namespace with DHCP + NAT and the eBPF VM edge),
    /// `user` (QEMU user-mode NAT, no tap and no eBPF) or `none`.
    #[serde(default)]
    pub fluxvm_network: String,
    /// Host NIC for a bridge-less TAP: FluxVM's TC/eBPF redirect moves frames between it
    /// and the tap. Mutually exclusive with `fluxvm_bridge`.
    #[serde(default)]
    pub fluxvm_direct_uplink: String,
    /// `l2-uplink` (default, physical/bond NIC) or `peer-veth` (CNI pod `eth0`).
    #[serde(default)]
    pub fluxvm_direct_mode: String,
    /// Guest IPv4s for `l2-uplink` ARP steering (at most 8).
    #[serde(default)]
    pub fluxvm_direct_guest_ips: Vec<String>,
    /// FluxVM direct-boot kernel on the host. Firecracker and flux-vm need one
    /// (FluxVM falls back to its own configured kernel); QEMU needs a bzImage.
    #[serde(default)]
    pub fluxvm_kernel: String,
    #[serde(default)]
    pub fluxvm_initrd: String,
    #[serde(default)]
    pub fluxvm_kernel_args: String,
    /// FluxVM in-guest vsock agent (exec, files, console). `None` = on.
    #[serde(default)]
    pub fluxvm_agent: Option<bool>,
    /// Use the image in place as a `storage: shared` disk (never cloned or deleted;
    /// needed for FluxVM HA re-create and live migration).
    #[serde(default)]
    pub fluxvm_shared_disk: bool,
    /// Install ISOs on the host, attached read-only as SATA CD-ROMs (QEMU only, at
    /// most 4). Drives are named `install`, `cd2`, `cd3`, `cd4`; eject them before
    /// live migration.
    #[serde(default)]
    pub fluxvm_isos: Vec<String>,
}

fn default_graphics_listen() -> String {
    "127.0.0.1".to_string()
}

fn default_graphics_type() -> String {
    "both".to_string()
}

fn default_template_disk_mode() -> String {
    "backing".to_string()
}

fn default_firmware() -> String {
    "bios".to_string()
}

fn default_vcpus() -> u32 {
    1
}
fn default_memory() -> u64 {
    1024
}
fn default_disk_gb() -> u64 {
    10
}
fn default_network() -> String {
    "default".to_string()
}
fn default_os_variant() -> String {
    "generic".to_string()
}

impl Default for CreateVmRequest {
    fn default() -> Self {
        Self {
            name: String::new(),
            vcpus: default_vcpus(),
            memory_mb: default_memory(),
            disk_gb: default_disk_gb(),
            iso: String::new(),
            network: default_network(),
            os_variant: default_os_variant(),
            guest_profile: String::new(),
            virtio_win_iso: String::new(),
            existing_disk: String::new(),
            firmware: default_firmware(),
            secure_boot: false,
            vtpm: false,
            graphics_listen: default_graphics_listen(),
            graphics_type: default_graphics_type(),
            cloud_init_iso: String::new(),
            cloud_init_user: String::new(),
            cloud_init_password: String::new(),
            cloud_init_ssh_pubkey: String::new(),
            saved_template: String::new(),
            template_disk_mode: default_template_disk_mode(),
            create_backend: String::new(),
            virt_builder_os: String::new(),
            virt_builder_hostname: String::new(),
            virt_builder_ssh_pubkey: String::new(),
            virt_builder_root_password_file: String::new(),
            virt_builder_packages: Vec::new(),
            virt_builder_firstboot_commands: Vec::new(),
            virt_builder_selinux_relabel: false,
            virt_builder_post_customize_install: Vec::new(),
            virt_builder_post_customize_run: Vec::new(),
            virt_builder_sysprep: false,
            mkosi_workspace: String::new(),
            mkosi_image: String::new(),
            virt_install_define_only: false,
            virt_install_location: String::new(),
            virt_install_pxe: false,
            virt_install_pxe_network: String::new(),
            virt_install_install_os: String::new(),
            virt_install_extra_args: String::new(),
            root_disk_storage_pool: String::new(),
            root_disk_storage_volume: String::new(),
            virt_install_path_in_use_check_off: false,
            virt_install_disk_backing_store: String::new(),
            virt_install_unattended: false,
            virt_install_admin_password: String::new(),
            virt_install_user_login: String::new(),
            virt_install_user_password: String::new(),
            libvirt_connection: String::new(),
            backend: String::new(),
            fluxvm_backend: String::new(),
            fluxvm_image: String::new(),
            fluxvm_bridge: String::new(),
            fluxvm_network: String::new(),
            fluxvm_direct_uplink: String::new(),
            fluxvm_direct_mode: String::new(),
            fluxvm_direct_guest_ips: Vec::new(),
            fluxvm_kernel: String::new(),
            fluxvm_initrd: String::new(),
            fluxvm_kernel_args: String::new(),
            fluxvm_agent: None,
            fluxvm_shared_disk: false,
            fluxvm_isos: Vec::new(),
        }
    }
}

// ── Network Create Request ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNetworkRequest {
    pub name: String,
    #[serde(default = "default_subnet")]
    pub subnet: String,
    #[serde(default = "default_dhcp_start")]
    pub dhcp_start: String,
    #[serde(default = "default_dhcp_end")]
    pub dhcp_end: String,
}

fn default_subnet() -> String {
    "192.168.100".to_string()
}
fn default_dhcp_start() -> String {
    "192.168.100.100".to_string()
}
fn default_dhcp_end() -> String {
    "192.168.100.254".to_string()
}

// ── Volume Create Request ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVolumeRequest {
    pub name: String,
    #[serde(default = "default_vol_capacity")]
    pub capacity_gb: u64,
    #[serde(default = "default_vol_format")]
    pub format: String,
}

fn default_vol_capacity() -> u64 {
    10
}
fn default_vol_format() -> String {
    "qcow2".to_string()
}

// ── Rename Request ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameVmRequest {
    pub new_name: String,
}

// ── Dashboard Stats ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct DashboardStats {
    pub total_vms: usize,
    pub running_vms: usize,
    pub stopped_vms: usize,
    pub paused_vms: usize,
    pub total_vcpus: u32,
    pub total_memory_mb: u64,
    pub used_memory_mb: u64,
    pub total_networks: usize,
    pub active_networks: usize,
    pub total_pools: usize,
    pub active_pools: usize,
    pub total_snapshots: usize,
}

// ── Disk Attach Request ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachDiskRequest {
    pub source: String,
    #[serde(default = "default_disk_target")]
    pub target: String,
    #[serde(default = "default_disk_driver")]
    pub driver: String,
    /// Disk bus for `<target … bus=…>` (default virtio).
    #[serde(default)]
    pub bus: String,
    #[serde(default)]
    pub cache: String,
    #[serde(default)]
    pub discard: String,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub shareable: bool,
}

fn default_disk_target() -> String {
    "vdb".to_string()
}
fn default_disk_driver() -> String {
    "qcow2".to_string()
}

// ── VM Templates ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmTemplate {
    pub name: String,
    pub description: String,
    pub vcpus: u32,
    pub memory_mb: u64,
    pub disk_gb: u64,
    pub os_variant: String,
    /// Golden image for fast clones: absolute path to a qcow2 (set when saving a template from a VM).
    #[serde(default)]
    pub base_image: Option<String>,
    /// When `base_image` is set: `backing` (default, `qemu-img create -b`) or `copy` (`qemu-img convert`).
    #[serde(default)]
    pub template_disk_mode: String,
}

impl VmTemplate {
    pub fn all() -> Vec<VmTemplate> {
        vec![
            VmTemplate {
                name: "linux-small".to_string(),
                description: "Linux VM: 1 vCPU, 1 GB RAM, 10 GB disk".to_string(),
                vcpus: 1,
                memory_mb: 1024,
                disk_gb: 10,
                os_variant: "linux2022".to_string(),
                base_image: None,
                template_disk_mode: String::new(),
            },
            VmTemplate {
                name: "linux-medium".to_string(),
                description: "Linux VM: 2 vCPUs, 4 GB RAM, 40 GB disk".to_string(),
                vcpus: 2,
                memory_mb: 4096,
                disk_gb: 40,
                os_variant: "linux2022".to_string(),
                base_image: None,
                template_disk_mode: String::new(),
            },
            VmTemplate {
                name: "linux-large".to_string(),
                description: "Linux VM: 4 vCPUs, 8 GB RAM, 80 GB disk".to_string(),
                vcpus: 4,
                memory_mb: 8192,
                disk_gb: 80,
                os_variant: "linux2022".to_string(),
                base_image: None,
                template_disk_mode: String::new(),
            },
            VmTemplate {
                name: "windows".to_string(),
                description: "Windows (dockur): 4 vCPUs, 8 GB RAM — use Golden Forge win11/win10 for base_image"
                    .to_string(),
                vcpus: 4,
                memory_mb: 8192,
                disk_gb: 64,
                os_variant: "win11".to_string(),
                base_image: Some("/var/lib/libvirt/images/win11.qcow2".to_string()),
                template_disk_mode: "copy".to_string(),
            },
            VmTemplate {
                name: "win11".to_string(),
                description: "Windows 11 dockur golden (UEFI + VirtIO, KubeVirt-ready)".to_string(),
                vcpus: 4,
                memory_mb: 8192,
                disk_gb: 64,
                os_variant: "win11".to_string(),
                base_image: Some("/var/lib/libvirt/images/win11.qcow2".to_string()),
                template_disk_mode: "copy".to_string(),
            },
            VmTemplate {
                name: "win10".to_string(),
                description: "Windows 10 dockur golden (UEFI + VirtIO, KubeVirt-ready)".to_string(),
                vcpus: 4,
                memory_mb: 8192,
                disk_gb: 64,
                os_variant: "win10".to_string(),
                base_image: Some("/var/lib/libvirt/images/win10.qcow2".to_string()),
                template_disk_mode: "copy".to_string(),
            },
            VmTemplate {
                name: "windows-server-2022".to_string(),
                description: "Windows Server 2022 dockur golden (UEFI + VirtIO, KubeVirt-ready)"
                    .to_string(),
                vcpus: 4,
                memory_mb: 8192,
                disk_gb: 80,
                os_variant: "win2k22".to_string(),
                base_image: Some("/var/lib/libvirt/images/windows-server-2022.qcow2".to_string()),
                template_disk_mode: "copy".to_string(),
            },
            VmTemplate {
                name: "windows-server-2025".to_string(),
                description: "Windows Server 2025 dockur golden (UEFI + VirtIO, KubeVirt-ready)"
                    .to_string(),
                vcpus: 4,
                memory_mb: 8192,
                disk_gb: 80,
                os_variant: "win2k25".to_string(),
                base_image: Some("/var/lib/libvirt/images/windows-server-2025.qcow2".to_string()),
                template_disk_mode: "copy".to_string(),
            },
            VmTemplate {
                name: "minimal".to_string(),
                description: "Minimal: 1 vCPU, 512 MB RAM, 5 GB disk".to_string(),
                vcpus: 1,
                memory_mb: 512,
                disk_gb: 5,
                os_variant: "linux2022".to_string(),
                base_image: None,
                template_disk_mode: String::new(),
            },
        ]
    }

    pub fn find(name: &str) -> Option<VmTemplate> {
        Self::all().into_iter().find(|t| t.name == name)
    }
}

// ── Notification Level ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyLevel {
    Success,
    Error,
    Warning,
    Info,
}

// ── Confirmation Dialog ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ConfirmationDialog {
    pub title: String,
    pub message: String,
    pub resource_name: String,
    pub action: String,
}

// ── Sidebar / Content Focus Model ───────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Sidebar,
    Content,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ObjectTab {
    #[default]
    Summary,
    Monitor,
    Configure,
}

impl ObjectTab {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Monitor => "Monitor",
            Self::Configure => "Configure",
        }
    }

    pub fn all() -> &'static [ObjectTab] {
        &[Self::Summary, Self::Monitor, Self::Configure]
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Summary => Self::Monitor,
            Self::Monitor => Self::Configure,
            Self::Configure => Self::Summary,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::Summary => Self::Configure,
            Self::Monitor => Self::Summary,
            Self::Configure => Self::Monitor,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidebarCategory {
    VirtualMachines,
    Networks,
    Storage,
    Snapshots,
    Backups,
}

impl SidebarCategory {
    pub fn all() -> &'static [SidebarCategory] {
        &[
            Self::VirtualMachines,
            Self::Networks,
            Self::Storage,
            Self::Snapshots,
            Self::Backups,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::VirtualMachines => "VMs",
            Self::Networks => "Networks",
            Self::Storage => "Storage",
            Self::Snapshots => "Snapshots",
            Self::Backups => "Backups",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarItem {
    Category(SidebarCategory),
    Vm(String),
    Network(String),
    StoragePool(String),
    Snapshot(String, String), // (vm_name, snap_name)
}

// ── TUI State ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResourceView {
    #[default]
    VirtualMachines,
    Networks,
    StoragePools,
    Snapshots,
    Backups,
    Events,
    Node,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Table,
    Xml,
    Logs,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Search,
    Confirmation,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortColumn {
    #[default]
    Name,
    State,
    Cpu,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn toggle(&self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }

    pub fn apply(&self, cmp: std::cmp::Ordering) -> std::cmp::Ordering {
        match self {
            Self::Ascending => cmp,
            Self::Descending => cmp.reverse(),
        }
    }
}

// ── Bounded buffer helper ───────────────────────────────────────────────

fn push_bounded<T>(buf: &mut VecDeque<T>, item: T, max: usize) {
    buf.push_back(item);
    if buf.len() > max {
        buf.pop_front();
    }
}

// ── App State ───────────────────────────────────────────────────────────

#[derive(Default)]
pub struct AppState {
    // Data
    pub vms: Vec<VmInfo>,
    pub networks: Vec<NetworkInfo>,
    pub storage_pools: Vec<StoragePoolInfo>,
    pub snapshots: Vec<SnapshotInfo>,
    pub node_info: Option<NodeInfo>,
    pub vm_details: Option<VmDetails>,
    pub vm_metrics: Vec<VmMetrics>,
    pub audit_events: VecDeque<AuditEvent>,
    pub xml_content: String,
    /// When non-empty, replaces the default title on XML / log full-screen overlays (e.g. KubeVirt YAML).
    pub content_overlay_caption: String,
    pub scroll_offset: u16,
    pub dashboard: DashboardStats,
    pub backups: Vec<BackupInfo>,
    pub volumes: Vec<StorageVolumeInfo>,
    pub browsing_pool: Option<String>,
    pub log_content: String,
    pub notification: Option<(String, Instant, NotifyLevel)>,
    pub notification_history: VecDeque<(String, NotifyLevel, String)>,

    // UI
    pub selected_index: usize,
    pub resource_view: ResourceView,
    pub view_mode: ViewMode,
    pub input_mode: InputMode,
    pub status_message: String,
    pub show_context_menu: bool,
    pub connected: bool,

    // Multi-select
    pub multi_select_mode: bool,
    pub selected_items: std::collections::HashSet<String>,

    // Search
    pub search_query: String,
    pub search_active: bool,
    pub filtered_indices: Vec<usize>,

    // Sort
    pub sort_column: SortColumn,
    pub sort_direction: SortDirection,

    // Confirmation
    pub confirm_dialog: Option<ConfirmationDialog>,

    // Command
    pub command_input: String,

    // VM state transitions
    pub previous_vm_states: HashMap<String, String>,
    pub state_changed_vms: HashMap<String, Instant>,

    // Metrics history (sparklines)
    pub metrics_history: HashMap<String, VecDeque<f64>>,

    // Sidebar + Content focus model
    pub focus: Focus,
    pub active_object_tab: ObjectTab,
    pub sidebar_selected: usize,
    pub sidebar_collapsed: HashMap<SidebarCategory, bool>,
    pub sidebar_items: Vec<SidebarItem>,
    pub content_scroll_offset: u16,
    pub command_content_override: Option<ResourceView>,
    pub help_scroll: u16,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            status_message: "Press '?' for help, ':' for commands".to_string(),
            ..Default::default()
        }
    }

    pub fn rebuild_sidebar(&mut self) {
        self.sidebar_items.clear();

        // Data-driven: each category maps to its child items
        let categories: Vec<(SidebarCategory, Vec<SidebarItem>)> = vec![
            (
                SidebarCategory::VirtualMachines,
                self.vms
                    .iter()
                    .map(|vm| SidebarItem::Vm(vm.name.clone()))
                    .collect(),
            ),
            (
                SidebarCategory::Networks,
                self.networks
                    .iter()
                    .map(|n| SidebarItem::Network(n.name.clone()))
                    .collect(),
            ),
            (
                SidebarCategory::Storage,
                self.storage_pools
                    .iter()
                    .map(|p| SidebarItem::StoragePool(p.name.clone()))
                    .collect(),
            ),
            (
                SidebarCategory::Snapshots,
                self.snapshots
                    .iter()
                    .map(|s| SidebarItem::Snapshot(s.vm_name.clone(), s.name.clone()))
                    .collect(),
            ),
            (SidebarCategory::Backups, vec![]),
        ];

        for (cat, children) in categories {
            self.sidebar_items.push(SidebarItem::Category(cat));
            if !self.is_collapsed(cat) {
                self.sidebar_items.extend(children);
            }
        }

        // Clamp sidebar selection
        if !self.sidebar_items.is_empty() && self.sidebar_selected >= self.sidebar_items.len() {
            self.sidebar_selected = self.sidebar_items.len() - 1;
        }
    }

    fn is_collapsed(&self, cat: SidebarCategory) -> bool {
        *self.sidebar_collapsed.get(&cat).unwrap_or(&false)
    }

    pub fn toggle_sidebar_collapse(&mut self) {
        if let Some(SidebarItem::Category(cat)) =
            self.sidebar_items.get(self.sidebar_selected).cloned()
        {
            let collapsed = self.is_collapsed(cat);
            self.sidebar_collapsed.insert(cat, !collapsed);
            self.rebuild_sidebar();
        }
    }

    pub fn selected_sidebar_item(&self) -> Option<&SidebarItem> {
        self.sidebar_items.get(self.sidebar_selected)
    }

    pub fn sidebar_resource_view(&self) -> ResourceView {
        match self.selected_sidebar_item() {
            Some(SidebarItem::Category(SidebarCategory::VirtualMachines))
            | Some(SidebarItem::Vm(_)) => ResourceView::VirtualMachines,
            Some(SidebarItem::Category(SidebarCategory::Networks))
            | Some(SidebarItem::Network(_)) => ResourceView::Networks,
            Some(SidebarItem::Category(SidebarCategory::Storage))
            | Some(SidebarItem::StoragePool(_)) => ResourceView::StoragePools,
            Some(SidebarItem::Category(SidebarCategory::Snapshots))
            | Some(SidebarItem::Snapshot(_, _)) => ResourceView::Snapshots,
            Some(SidebarItem::Category(SidebarCategory::Backups)) => ResourceView::Backups,
            None => ResourceView::VirtualMachines,
        }
    }

    pub fn sidebar_selected_name(&self) -> Option<&str> {
        match self.selected_sidebar_item() {
            Some(SidebarItem::Vm(name)) => Some(name.as_str()),
            Some(SidebarItem::Network(name)) => Some(name.as_str()),
            Some(SidebarItem::StoragePool(name)) => Some(name.as_str()),
            Some(SidebarItem::Snapshot(_, name)) => Some(name.as_str()),
            _ => None,
        }
    }

    pub fn effective_vm_name(&self) -> Option<&str> {
        match self.selected_sidebar_item() {
            Some(SidebarItem::Vm(name)) => Some(name.as_str()),
            Some(SidebarItem::Category(SidebarCategory::VirtualMachines)) => {
                self.selected_vm_name()
            }
            _ => None,
        }
    }

    pub fn effective_network_name(&self) -> Option<&str> {
        match self.selected_sidebar_item() {
            Some(SidebarItem::Network(name)) => Some(name.as_str()),
            Some(SidebarItem::Category(SidebarCategory::Networks)) => self.selected_network_name(),
            _ => None,
        }
    }

    pub fn effective_pool_name(&self) -> Option<&str> {
        match self.selected_sidebar_item() {
            Some(SidebarItem::StoragePool(name)) => Some(name.as_str()),
            Some(SidebarItem::Category(SidebarCategory::Storage)) => self.selected_pool_name(),
            _ => None,
        }
    }

    pub fn effective_snapshot(&self) -> Option<&SnapshotInfo> {
        match self.selected_sidebar_item() {
            Some(SidebarItem::Snapshot(vm, snap)) => self.find_snapshot(vm, snap),
            Some(SidebarItem::Category(SidebarCategory::Snapshots)) => self.selected_snapshot(),
            _ => None,
        }
    }

    pub fn add_audit_event(&mut self, action: &str, target: &str, result: &str) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let event = AuditEvent {
            timestamp: now,
            action: action.to_string(),
            target: target.to_string(),
            result: result.to_string(),
            actor: String::new(),
        };
        crate::audit::write_audit_event(&event);
        push_bounded(&mut self.audit_events, event, 500);
    }

    pub fn compute_dashboard(&mut self) {
        let running_vms = self.vms.iter().filter(|v| v.state == "running").count();
        let paused_vms = self.vms.iter().filter(|v| v.state == "paused").count();
        let stopped_vms = self.vms.len() - running_vms - paused_vms;
        let total_vcpus: u32 = self.vms.iter().map(|v| v.vcpus).sum();
        let total_memory_mb: u64 = self.vms.iter().map(|v| v.memory_mb).sum();
        let used_memory_mb: u64 = self.vm_metrics.iter().map(|m| m.memory_used_mb).sum();

        self.dashboard = DashboardStats {
            total_vms: self.vms.len(),
            running_vms,
            stopped_vms,
            paused_vms,
            total_vcpus,
            total_memory_mb,
            used_memory_mb,
            total_networks: self.networks.len(),
            active_networks: self.networks.iter().filter(|n| n.active).count(),
            total_pools: self.storage_pools.len(),
            active_pools: self
                .storage_pools
                .iter()
                .filter(|p| p.state == "running")
                .count(),
            total_snapshots: self.snapshots.len(),
        };
    }

    pub fn notify(&mut self, msg: &str) {
        let level = if msg.contains("Error") || msg.contains("ERROR") || msg.contains("failed") {
            NotifyLevel::Error
        } else if msg.contains("warn") || msg.contains("WARN") {
            NotifyLevel::Warning
        } else {
            NotifyLevel::Success
        };
        self.notify_with_level(msg, level);
    }

    pub fn notify_with_level(&mut self, msg: &str, level: NotifyLevel) {
        self.notification = Some((msg.to_string(), Instant::now(), level));
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
        push_bounded(
            &mut self.notification_history,
            (msg.to_string(), level, timestamp),
            100,
        );
    }

    pub fn load_audit_history(&mut self) {
        self.audit_events = crate::audit::load_audit_events(500).into();
    }

    pub fn find_vm(&self, name: &str) -> Option<&VmInfo> {
        self.vms.iter().find(|v| v.name == name)
    }

    pub fn vm_state_str(&self, name: &str) -> &str {
        self.find_vm(name)
            .map(|v| v.state.as_str())
            .unwrap_or("unknown")
    }

    pub fn find_network(&self, name: &str) -> Option<&NetworkInfo> {
        self.networks.iter().find(|n| n.name == name)
    }

    pub fn find_pool(&self, name: &str) -> Option<&StoragePoolInfo> {
        self.storage_pools.iter().find(|p| p.name == name)
    }

    pub fn find_snapshot(&self, vm_name: &str, snap_name: &str) -> Option<&SnapshotInfo> {
        self.snapshots
            .iter()
            .find(|s| s.vm_name == vm_name && s.name == snap_name)
    }

    pub fn get_metrics_for_vm(&self, name: &str) -> Option<&VmMetrics> {
        self.vm_metrics.iter().find(|m| m.name == name)
    }

    pub fn toggle_selection(&mut self, name: &str) {
        if self.selected_items.contains(name) {
            self.selected_items.remove(name);
        } else {
            self.selected_items.insert(name.to_string());
        }
    }

    pub fn select_all_vms(&mut self) {
        for vm in &self.vms {
            self.selected_items.insert(vm.name.clone());
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected_items.clear();
        self.multi_select_mode = false;
    }

    pub fn current_list_len(&self) -> usize {
        if self.search_active {
            return self.filtered_indices.len();
        }
        match self.resource_view {
            ResourceView::VirtualMachines => self.vms.len(),
            ResourceView::Networks => self.networks.len(),
            ResourceView::StoragePools => self.storage_pools.len(),
            ResourceView::Snapshots => self.snapshots.len(),
            ResourceView::Backups => self.backups.len(),
            ResourceView::Events => self.audit_events.len(),
            ResourceView::Node => 1,
        }
    }

    pub fn clamp_selection(&mut self) {
        let len = self.current_list_len();
        if len == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= len {
            self.selected_index = len - 1;
        }
    }

    pub fn selected_vm_name(&self) -> Option<&str> {
        if self.resource_view != ResourceView::VirtualMachines {
            return None;
        }
        let idx = if self.search_active {
            *self.filtered_indices.get(self.selected_index)?
        } else {
            self.selected_index
        };
        self.vms.get(idx).map(|vm| vm.name.as_str())
    }

    pub fn selected_network_name(&self) -> Option<&str> {
        if self.resource_view != ResourceView::Networks {
            return None;
        }
        self.networks
            .get(self.selected_index)
            .map(|n| n.name.as_str())
    }

    pub fn selected_pool_name(&self) -> Option<&str> {
        if self.resource_view != ResourceView::StoragePools {
            return None;
        }
        self.storage_pools
            .get(self.selected_index)
            .map(|p| p.name.as_str())
    }

    pub fn selected_snapshot(&self) -> Option<&SnapshotInfo> {
        if self.resource_view != ResourceView::Snapshots {
            return None;
        }
        self.snapshots.get(self.selected_index)
    }

    pub fn apply_search_filter(&mut self) {
        if self.search_query.is_empty() {
            self.search_active = false;
            self.filtered_indices.clear();
            return;
        }
        self.search_active = true;
        let query = self.search_query.to_lowercase();

        let mut scored = match self.resource_view {
            ResourceView::VirtualMachines => score_searchable(&self.vms, &query),
            ResourceView::Networks => score_searchable(&self.networks, &query),
            ResourceView::StoragePools => score_searchable(&self.storage_pools, &query),
            ResourceView::Snapshots => score_searchable(&self.snapshots, &query),
            ResourceView::Backups | ResourceView::Events | ResourceView::Node => vec![],
        };

        scored.sort_by_key(|x| std::cmp::Reverse(x.1));
        self.filtered_indices = scored.into_iter().map(|(i, _)| i).collect();
        self.clamp_selection();
    }

    pub fn detect_state_changes(&mut self) {
        self.state_changed_vms
            .retain(|_, when| when.elapsed().as_secs() < 3);

        for vm in &self.vms {
            if let Some(prev_state) = self.previous_vm_states.get(&vm.name) {
                if *prev_state != vm.state {
                    self.state_changed_vms
                        .insert(vm.name.clone(), Instant::now());
                }
            }
        }

        self.previous_vm_states.clear();
        for vm in &self.vms {
            self.previous_vm_states
                .insert(vm.name.clone(), vm.state.clone());
        }
    }

    pub fn record_metrics_snapshot(&mut self) {
        for m in &self.vm_metrics {
            let history = self.metrics_history.entry(m.name.clone()).or_default();
            push_bounded(history, m.memory_pct, 20);
        }
    }

    pub fn sort_vms(&mut self) {
        let dir = self.sort_direction;
        match self.sort_column {
            SortColumn::Name => self.vms.sort_by(|a, b| dir.apply(a.name.cmp(&b.name))),
            SortColumn::State => self.vms.sort_by(|a, b| dir.apply(a.state.cmp(&b.state))),
            SortColumn::Cpu => self.vms.sort_by(|a, b| dir.apply(a.vcpus.cmp(&b.vcpus))),
            SortColumn::Memory => self
                .vms
                .sort_by(|a, b| dir.apply(a.memory_mb.cmp(&b.memory_mb))),
        }
    }
}

// ── Searchable trait ────────────────────────────────────────────────────

trait Searchable {
    fn search_fields(&self) -> Vec<String>;
}

impl Searchable for VmInfo {
    fn search_fields(&self) -> Vec<String> {
        vec![self.name.to_lowercase(), self.state.to_lowercase()]
    }
}

impl Searchable for NetworkInfo {
    fn search_fields(&self) -> Vec<String> {
        vec![self.name.to_lowercase()]
    }
}

impl Searchable for StoragePoolInfo {
    fn search_fields(&self) -> Vec<String> {
        vec![self.name.to_lowercase()]
    }
}

impl Searchable for SnapshotInfo {
    fn search_fields(&self) -> Vec<String> {
        vec![self.name.to_lowercase(), self.vm_name.to_lowercase()]
    }
}

fn score_searchable<T: Searchable>(items: &[T], query: &str) -> Vec<(usize, i32)> {
    items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let best = item
                .search_fields()
                .iter()
                .map(|f| fuzzy_match(f, query))
                .max()
                .unwrap_or(0);
            if best > 0 {
                Some((i, best))
            } else {
                None
            }
        })
        .collect()
}

/// Fuzzy match: all query characters must appear in order in the target.
/// Returns a score > 0 on match, 0 on no match.
/// Consecutive matches and prefix matches score higher.
pub fn fuzzy_match(target: &str, query: &str) -> i32 {
    let target_chars: Vec<char> = target.chars().collect();
    let query_chars: Vec<char> = query.chars().collect();

    if query_chars.is_empty() {
        return 1;
    }
    if query_chars.len() > target_chars.len() {
        return 0;
    }

    // Exact substring match gets highest score
    if target.contains(query) {
        return 100 + (query_chars.len() as i32 * 10);
    }

    let mut score = 0i32;
    let mut ti = 0;
    let mut prev_match = false;
    let mut consecutive = 0;

    for &qc in &query_chars {
        let mut found = false;
        while ti < target_chars.len() {
            if target_chars[ti] == qc {
                found = true;
                score += 1;
                if ti == 0 {
                    score += 5; // prefix bonus
                }
                if prev_match {
                    consecutive += 1;
                    score += consecutive * 2; // consecutive bonus
                } else {
                    consecutive = 0;
                }
                prev_match = true;
                ti += 1;
                break;
            }
            prev_match = false;
            ti += 1;
        }
        if !found {
            return 0; // query char not found
        }
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match_exact() {
        assert!(fuzzy_match("myvm", "myvm") > 0);
    }

    #[test]
    fn test_fuzzy_match_substring() {
        let score = fuzzy_match("my-test-vm", "test");
        assert!(score >= 100);
    }

    #[test]
    fn test_fuzzy_match_no_match() {
        assert_eq!(fuzzy_match("abc", "xyz"), 0);
    }

    #[test]
    fn test_fuzzy_match_empty_query() {
        assert_eq!(fuzzy_match("anything", ""), 1);
    }

    #[test]
    fn test_fuzzy_match_query_longer_than_target() {
        assert_eq!(fuzzy_match("ab", "abc"), 0);
    }

    #[test]
    fn test_fuzzy_match_fuzzy_order() {
        // "mxvx" matches "mv" fuzzy but not as substring
        let score_a = fuzzy_match("mvhost", "mv");
        let score_b = fuzzy_match("m-x-v-host", "mv");
        // substring match scores higher than fuzzy
        assert!(score_a > score_b);
    }

    #[test]
    fn test_search_filter_no_matches_returns_zero() {
        let mut state = AppState::new();
        state.resource_view = ResourceView::VirtualMachines;
        state.vms = vec![
            VmInfo {
                name: "alpha".into(),
                state: "running".into(),
                vcpus: 1,
                memory_mb: 512,
                libvirt_connection: None,
                guest_ip: None,
                guest_ips: Vec::new(),
                backend: None,
                fluxvm_backend: None,
            },
            VmInfo {
                name: "bravo".into(),
                state: "shutoff".into(),
                vcpus: 2,
                memory_mb: 1024,
                libvirt_connection: None,
                guest_ip: None,
                guest_ips: Vec::new(),
                backend: None,
                fluxvm_backend: None,
            },
        ];
        state.search_query = "zzzznotfound".into();
        state.apply_search_filter();
        assert!(state.search_active);
        assert_eq!(state.current_list_len(), 0);
    }

    #[test]
    fn test_search_filter_empty_query_shows_all() {
        let mut state = AppState::new();
        state.resource_view = ResourceView::VirtualMachines;
        state.vms = vec![VmInfo {
            name: "alpha".into(),
            state: "running".into(),
            vcpus: 1,
            memory_mb: 512,
            libvirt_connection: None,
            guest_ip: None,
            guest_ips: Vec::new(),
            backend: None,
            fluxvm_backend: None,
        }];
        state.search_query.clear();
        state.apply_search_filter();
        assert!(!state.search_active);
        assert_eq!(state.current_list_len(), 1);
    }

    #[test]
    fn test_search_filter_with_matches() {
        let mut state = AppState::new();
        state.resource_view = ResourceView::VirtualMachines;
        state.vms = vec![
            VmInfo {
                name: "alpha".into(),
                state: "running".into(),
                vcpus: 1,
                memory_mb: 512,
                libvirt_connection: None,
                guest_ip: None,
                guest_ips: Vec::new(),
                backend: None,
                fluxvm_backend: None,
            },
            VmInfo {
                name: "bravo".into(),
                state: "shutoff".into(),
                vcpus: 2,
                memory_mb: 1024,
                libvirt_connection: None,
                guest_ip: None,
                guest_ips: Vec::new(),
                backend: None,
                fluxvm_backend: None,
            },
            VmInfo {
                name: "charlie".into(),
                state: "running".into(),
                vcpus: 1,
                memory_mb: 512,
                libvirt_connection: None,
                guest_ip: None,
                guest_ips: Vec::new(),
                backend: None,
                fluxvm_backend: None,
            },
        ];
        state.search_query = "alpha".into();
        state.apply_search_filter();
        assert!(state.search_active);
        assert_eq!(state.current_list_len(), 1);
    }

    #[test]
    fn test_dashboard_compute() {
        let mut state = AppState::new();
        state.vms = vec![
            VmInfo {
                name: "a".into(),
                state: "running".into(),
                vcpus: 2,
                memory_mb: 1024,
                libvirt_connection: None,
                guest_ip: None,
                guest_ips: Vec::new(),
                backend: None,
                fluxvm_backend: None,
            },
            VmInfo {
                name: "b".into(),
                state: "shutoff".into(),
                vcpus: 1,
                memory_mb: 512,
                libvirt_connection: None,
                guest_ip: None,
                guest_ips: Vec::new(),
                backend: None,
                fluxvm_backend: None,
            },
            VmInfo {
                name: "c".into(),
                state: "paused".into(),
                vcpus: 4,
                memory_mb: 2048,
                libvirt_connection: None,
                guest_ip: None,
                guest_ips: Vec::new(),
                backend: None,
                fluxvm_backend: None,
            },
        ];
        state.compute_dashboard();
        assert_eq!(state.dashboard.total_vms, 3);
        assert_eq!(state.dashboard.running_vms, 1);
        assert_eq!(state.dashboard.stopped_vms, 1);
        assert_eq!(state.dashboard.paused_vms, 1);
        assert_eq!(state.dashboard.total_vcpus, 7);
        assert_eq!(state.dashboard.total_memory_mb, 3584);
    }

    #[test]
    fn test_sort_vms() {
        let mut state = AppState::new();
        state.vms = vec![
            VmInfo {
                name: "charlie".into(),
                state: "running".into(),
                vcpus: 1,
                memory_mb: 512,
                libvirt_connection: None,
                guest_ip: None,
                guest_ips: Vec::new(),
                backend: None,
                fluxvm_backend: None,
            },
            VmInfo {
                name: "alpha".into(),
                state: "shutoff".into(),
                vcpus: 2,
                memory_mb: 1024,
                libvirt_connection: None,
                guest_ip: None,
                guest_ips: Vec::new(),
                backend: None,
                fluxvm_backend: None,
            },
            VmInfo {
                name: "bravo".into(),
                state: "paused".into(),
                vcpus: 4,
                memory_mb: 256,
                libvirt_connection: None,
                guest_ip: None,
                guest_ips: Vec::new(),
                backend: None,
                fluxvm_backend: None,
            },
        ];
        state.sort_column = SortColumn::Name;
        state.sort_direction = SortDirection::Ascending;
        state.sort_vms();
        assert_eq!(state.vms[0].name, "alpha");
        assert_eq!(state.vms[2].name, "charlie");

        state.sort_direction = SortDirection::Descending;
        state.sort_vms();
        assert_eq!(state.vms[0].name, "charlie");
    }

    #[test]
    fn test_push_bounded() {
        let mut buf = std::collections::VecDeque::new();
        for i in 0..5 {
            push_bounded(&mut buf, i, 3);
        }
        assert_eq!(buf.len(), 3);
        assert_eq!(buf[0], 2);
        assert_eq!(buf[2], 4);
    }
}
