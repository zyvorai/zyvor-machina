// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { apiPost, apiPostVoid, apiDelete, readJsonArray, readJsonObject, apiGetText } from './client'
import type { VmCgroupStats } from './extras'
import { streamResponseError } from './streamResponseError'
import { formatUserError } from '../utils/apiError'

const API = '/api/v1'

export interface VmInfo {
  name: string
  state: string
  vcpus: number
  memory_mb: number
  /** Present when daemon uses dual `qemu:///system` + `qemu:///session`. */
  libvirt_connection?: string
  /** Best-effort IPv4 from libvirt lease / ARP / guest agent. */
  guest_ip?: string | null
  /** `fluxvm` for VMs managed by fluxvm-api; absent for libvirt domains. */
  backend?: string
  /** FluxVM hypervisor: `qemu`, `cloud-hypervisor`, `firecracker`, `flux-vm`. */
  fluxvm_backend?: string
}

/**
 * FluxVM VMs ride the existing `connection` plumbing: rows carry
 * `libvirt_connection: 'fluxvm'`, and every URL helper turns that into `?backend=fluxvm`.
 */
export const FLUXVM_CONNECTION = 'fluxvm'

export function isFluxvmConnection(connection?: string | null): boolean {
  return connection === FLUXVM_CONNECTION
}

/** Query param for a VM scope: '' (system), `connection=session`, or `backend=fluxvm`. */
export function vmScopeParam(connection?: string | null): string {
  if (!connection || connection === 'system') return ''
  if (isFluxvmConnection(connection)) return 'backend=fluxvm'
  return `connection=${encodeURIComponent(connection)}`
}

/** Best-effort parse so list UIs never throw if `/vms` returns unexpected shapes (proxy bugs, partial JSON). */
export function sanitizeVmInfo(row: unknown): VmInfo | null {
  if (!row || typeof row !== 'object') return null
  const r = row as Record<string, unknown>
  const nameRaw = r.name
  const name =
    typeof nameRaw === 'string'
      ? nameRaw
      : nameRaw != null && String(nameRaw).trim() !== ''
        ? String(nameRaw)
        : ''
  if (!name.trim()) return null
  const stateRaw = r.state
  const state =
    typeof stateRaw === 'string'
      ? stateRaw
      : stateRaw != null
        ? String(stateRaw)
        : 'unknown'
  let vcpus = 0
  if (typeof r.vcpus === 'number' && Number.isFinite(r.vcpus)) {
    vcpus = Math.max(0, Math.floor(r.vcpus))
  } else if (r.vcpus != null) {
    const n = Number(r.vcpus)
    if (Number.isFinite(n)) vcpus = Math.max(0, Math.floor(n))
  }
  let memory_mb = 0
  if (typeof r.memory_mb === 'number' && Number.isFinite(r.memory_mb)) {
    memory_mb = Math.max(0, Math.floor(r.memory_mb))
  } else if (r.memory_mb != null) {
    const n = Number(r.memory_mb)
    if (Number.isFinite(n)) memory_mb = Math.max(0, Math.floor(n))
  }
  const conn = r.libvirt_connection
  const libvirt_connection =
    typeof conn === 'string' && conn.length > 0 ? conn : undefined
  const vm: VmInfo = { name, state, vcpus, memory_mb }
  if (libvirt_connection) vm.libvirt_connection = libvirt_connection
  const guestIpRaw = r.guest_ip
  if (typeof guestIpRaw === 'string' && guestIpRaw.trim()) vm.guest_ip = guestIpRaw.trim()
  if (r.backend === FLUXVM_CONNECTION) {
    vm.backend = FLUXVM_CONNECTION
    vm.libvirt_connection = FLUXVM_CONNECTION
    if (typeof r.fluxvm_backend === 'string' && r.fluxvm_backend) vm.fluxvm_backend = r.fluxvm_backend
  }
  return vm
}

export interface VmDetails {
  name: string
  uuid: string
  state: string
  vcpus: number
  memory_mb: number
  os_type: string
  arch: string
  autostart: boolean
  persistent: boolean
  interfaces: InterfaceInfo[]
  disks: DiskInfo[]
  filesystems?: FilesystemInfo[]
  libvirt_connection?: string
  /** Best-effort IPv4 from libvirt lease / ARP / guest agent. */
  guest_ip?: string | null
  backend?: string
  fluxvm_backend?: string
}

/** Append the VM scope (`connection=` or `backend=fluxvm`) to a path or URL that may already have a query. */
export function appendVmConnection(url: string, connection?: string | null): string {
  const param = vmScopeParam(connection)
  if (!param) return url
  const sep = url.includes('?') ? '&' : '?'
  return `${url}${sep}${param}`
}

/** In-app route query: `?connection=session|fluxvm` (the UI's scope; API calls go through `appendVmConnection`). */
function appendRouteConnection(url: string, connection?: string | null): string {
  if (!connection || connection === 'system') return url
  const sep = url.includes('?') ? '&' : '?'
  return `${url}${sep}connection=${encodeURIComponent(connection)}`
}

/** In-app routes for a guest, with optional `?connection=session` (dual libvirt) or `?connection=fluxvm`. */
export function vmDetailRoute(name: string, connection?: string | null): string {
  return appendRouteConnection(`/vms/${encodeURIComponent(name)}`, connection)
}

/** FluxVM guests have no libvirt console plan; their console is the serial tab on the detail page. */
export function vmConsoleRoute(name: string, connection?: string | null): string {
  if (isFluxvmConnection(connection)) return `${vmDetailRoute(name, connection)}&tab=serial`
  return appendRouteConnection(`/vms/${encodeURIComponent(name)}/consolehub`, connection)
}

/** Row / selection key when system and session guests can share the same name. */
export function vmScopeKey(vm: Pick<VmInfo, 'name' | 'libvirt_connection'>): string {
  return `${vm.libvirt_connection ?? 'system'}::${vm.name}`
}

interface InterfaceInfo {
  mac_address: string
  source: string
  model: string
}

interface DiskInfo {
  device: string
  source: string
  driver: string
  target: string
  bus?: string
  cache?: string
  readonly?: boolean
  shareable?: boolean
}

interface FilesystemInfo {
  source: string
  mount_tag: string
  driver?: string
  accessmode?: string
  xattr?: boolean
}

interface VmBlockDeviceMetrics {
  device: string
  rd_bytes: number
  wr_bytes: number
  rd_ops: number
  wr_ops: number
}

interface VmNetDeviceMetrics {
  device: string
  rx_bytes: number
  tx_bytes: number
  rx_packets: number
  tx_packets: number
}

interface VmVcpuMetrics {
  vcpu: number
  cpu_time_ns: number
  state: string
}

export interface VmMetrics {
  name: string
  state?: string
  running?: boolean
  cpu_time_ns: number
  vcpus: number
  memory_total_mb: number
  memory_used_mb: number
  memory_pct: number
  disk_rd_bytes: number
  disk_wr_bytes: number
  disk_rd_ops?: number
  disk_wr_ops?: number
  net_rx_bytes: number
  net_tx_bytes: number
  vcpus_detail?: VmVcpuMetrics[]
  disks?: VmBlockDeviceMetrics[]
  nets?: VmNetDeviceMetrics[]
  cgroup?: VmCgroupStats | null
  /** When dual libvirt is enabled (`system` | `session`). */
  libvirt_connection?: string | null
}

export interface CreateVmRequest {
  name: string
  vcpus: number
  memory_mb: number
  disk_gb: number
  iso?: string
  network?: string
  os_variant?: string
  /** `auto` (default) | `linux` | `windows` */
  guest_profile?: string
  /** Optional virtio-win driver ISO to attach as extra CD-ROM (Windows installs). */
  virtio_win_iso?: string
  existing_disk?: string
  firmware?: string
  /** Enforce UEFI Secure Boot with keys enrolled (needs firmware 'uefi'). */
  secure_boot?: boolean
  /** Add an emulated TPM 2.0 (swtpm). */
  vtpm?: boolean
  /** Libvirt VNC listen IP (default 127.0.0.1). Use 0.0.0.0 for all interfaces (remote display; still use machina’s console proxy where applicable). */
  graphics_listen?: string
  /** `vnc` (noVNC) or `spice` (spice-html5). */
  graphics_type?: string
  /** Second CD-ROM: cloud-init / seed ISO (install ISO stays in `iso`). */
  cloud_init_iso?: string
  /** If set (and `cloud_init_iso` is empty), machina generates a NoCloud seed ISO and attaches it. */
  cloud_init_user?: string
  /** Plaintext password to pass via cloud-init `chpasswd`. */
  cloud_init_password?: string
  /** Single-line OpenSSH public key to inject for the cloud-init user. */
  cloud_init_ssh_pubkey?: string
  /** Saved template key under `/var/lib/machina/templates/` (server merges + optional golden disk). */
  saved_template?: string
  /** With saved template + `base_image`: `backing` (default) or `copy`. */
  template_disk_mode?: string
  /** `libvirt_xml` | `virt_install` | omit for server default from config. */
  create_backend?: string
  /** Legacy: libguestfs `virt-builder` template name. Server default is **disabled** (`[libvirt] virt_builder_allowed`); prefer `mkosi_workspace`. */
  virt_builder_os?: string
  virt_builder_hostname?: string
  virt_builder_ssh_pubkey?: string
  /** Absolute path on the **host** to a small file (≤4KiB) with the guest root password; passed as `virt-builder --root-password file:…`. */
  virt_builder_root_password_file?: string
  virt_builder_packages?: string[]
  virt_builder_firstboot_commands?: string[]
  virt_builder_selinux_relabel?: boolean
  virt_builder_post_customize_install?: string[]
  virt_builder_post_customize_run?: string[]
  virt_builder_sysprep?: boolean
  /** Preferred: absolute directory with `mkosi.conf`; runs `mkosi build`. Mutually exclusive with `virt_builder_os`. */
  mkosi_workspace?: string
  /** For multi-image mkosi workspaces (image trees): selects one image via `--image <name>`. */
  mkosi_image?: string
  /** `virt-install --print-xml=1` + define; halted shell (no install media). Requires virt_install backend. */
  virt_install_define_only?: boolean
  /** Network / kickstart tree: `virt-install --location …`. */
  virt_install_location?: string
  /** `virt-install --pxe` (extra NIC; network from `virt_install_pxe_network` or `network`). */
  virt_install_pxe?: boolean
  /** Libvirt network for the PXE interface (defaults to main `network`). */
  virt_install_pxe_network?: string
  /** `virt-install --install os=…` (libosinfo short id). */
  virt_install_install_os?: string
  /** `virt-install --extra-args`. */
  virt_install_extra_args?: string
  /** With `root_disk_storage_volume`: `virt-install --disk vol=pool/vol`. */
  root_disk_storage_pool?: string
  root_disk_storage_volume?: string
  /** `virt-install --check path_in_use=off`. */
  virt_install_path_in_use_check_off?: boolean
  /** New overlay root disk with `backing_store=` + `--import`. */
  virt_install_disk_backing_store?: string
  /** When `[libvirt] dual_connection`: `system` (default) or `session` — where the domain is defined. */
  libvirt_connection?: string
  /** `fluxvm` creates the guest through fluxvm-api (`[fluxvm] enabled`); omit for libvirt. */
  backend?: string
  /** FluxVM hypervisor: `auto` | `qemu` | `cloud-hypervisor` | `firecracker` | `flux-vm`. */
  fluxvm_backend?: string
  /** Base image path on the FluxVM host. */
  fluxvm_image?: string
  /** Host bridge for the tap (FluxVM attaches its eBPF VM edge to the tap). */
  fluxvm_bridge?: string
  /** Without a bridge or uplink: `netns` (default: tap in its own namespace + eBPF edge), `user` (QEMU only), `none`. */
  fluxvm_network?: string
  /** Host NIC for a bridge-less tap (FluxVM TC/eBPF redirect). */
  fluxvm_direct_uplink?: string
  /** `l2-uplink` (default) or `peer-veth`. */
  fluxvm_direct_mode?: string
  /** Guest IPv4s for `l2-uplink` ARP steering (at most 8). */
  fluxvm_direct_guest_ips?: string[]
  /** Kernel / initrd paths on the FluxVM host and extra kernel args (direct boot). */
  fluxvm_kernel?: string
  fluxvm_initrd?: string
  fluxvm_kernel_args?: string
  /** FluxVM guest agent (agent console, exec); on unless false. */
  fluxvm_agent?: boolean
  /** Use `fluxvm_image` in place as a shared raw disk (`storage: shared`): needed for live migration and HA. */
  fluxvm_shared_disk?: boolean
  /** Install ISOs on the FluxVM host, attached as CD-ROMs `install`, `cd2`… (QEMU only, at most 4). */
  fluxvm_isos?: string[]
}

export interface VmTemplate {
  name: string
  description: string
  vcpus: number
  memory_mb: number
  disk_gb: number
  os_variant: string
  /** Present on saved templates: golden qcow2 for backing-file clones. */
  base_image?: string | null
  template_disk_mode?: string
}

export async function listVMs(): Promise<VmInfo[]> {
  const raw = await readJsonArray<unknown>(`${API}/vms`)
  if (!Array.isArray(raw)) {
    console.warn('machina: GET /api/v1/vms expected a JSON array')
    return []
  }
  const out: VmInfo[] = []
  for (const row of raw) {
    const vm = sanitizeVmInfo(row)
    if (vm) out.push(vm)
  }
  return out
}
export const getVM = (name: string, connection?: string | null) =>
  readJsonObject<VmDetails>(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}`, connection)).then((d) =>
    d.backend === FLUXVM_CONNECTION ? { ...d, libvirt_connection: FLUXVM_CONNECTION } : d,
  )
export const getVMXml = (name: string, connection?: string | null) =>
  apiGetText(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/xml`, connection))

/** CDI DataVolume (upload) + KubeVirt VM YAML; virtio-win CDROM via containerDisk (post-migrate driver disk pattern). */
export interface KubeVirtBundle {
  libvirt_vm: string
  libvirt_root_disk: string
  /** `linux` or `windows` */
  guest_os?: string
  namespace: string
  virtual_machine_name: string
  datavolume_name: string
  /** PVC/DataVolume upload size (Gi) for virtctl and manifests. */
  upload_size_gi: number
  /** True when `[kubevirt] exec_enabled` — daemon may run kubectl/virtctl for this VM. */
  cluster_exec_enabled: boolean
  yaml: string
  virtctl_image_upload_example: string
}

/** JSON from `POST .../kubevirt/{apply,upload,start}` when the command finished (check `exit_code`). */
export interface KubeVirtClusterExecResult {
  exit_code: number
  stdout: string
  stderr: string
}

type KubeVirtBundleQuery = {
  namespace?: string
  k8s_vm_name?: string
  datavolume_name?: string
  storage_gi?: number
  storage_class?: string
  /** Default true: attach virtio-win as containerDisk CDROM. */
  include_virtio_cdrom?: boolean
  /** `session` when dual libvirt — domain lives on qemu:///session. */
  connection?: string
}

export function getKubeVirtBundle(name: string, q?: KubeVirtBundleQuery) {
  const p = new URLSearchParams()
  if (q?.namespace) p.set('namespace', q.namespace)
  if (q?.k8s_vm_name) p.set('k8s_vm_name', q.k8s_vm_name)
  if (q?.datavolume_name) p.set('datavolume_name', q.datavolume_name)
  if (q?.storage_gi != null) p.set('storage_gi', String(q.storage_gi))
  if (q?.storage_class) p.set('storage_class', q.storage_class)
  if (q?.include_virtio_cdrom === false) p.set('include_virtio_cdrom', 'false')
  if (q?.connection) p.set('connection', q.connection)
  const qs = p.toString()
  return readJsonObject<KubeVirtBundle>(
    `${API}/vms/${encodeURIComponent(name)}/kubevirt-bundle${qs ? `?${qs}` : ''}`,
  )
}

/** POST body for kubevirt apply/upload/start — same fields as `KubeVirtBundleQuery` (all optional). */
export type KubeVirtBundleBody = KubeVirtBundleQuery

export function postKubeVirtApply(name: string, body: KubeVirtBundleBody = {}) {
  return apiPost<KubeVirtClusterExecResult>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/kubevirt/apply`, body.connection),
    body,
  )
}

export function postKubeVirtUpload(name: string, body: KubeVirtBundleBody = {}) {
  return apiPost<KubeVirtClusterExecResult>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/kubevirt/upload`, body.connection),
    body,
  )
}

export function postKubeVirtStart(name: string, body: KubeVirtBundleBody = {}) {
  return apiPost<KubeVirtClusterExecResult>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/kubevirt/start`, body.connection),
    body,
  )
}

interface CreateVmStreamResult {
  status: string
  name: string
}

/** Create VM with live log lines (mkosi / virt-builder / virt-install / qemu-img) via SSE (`POST /vms/stream`). */
export async function createVMWithProgress(
  req: CreateVmRequest,
  onLogLine: (line: string) => void,
  onJobRegistered?: (job: { id: string }) => void,
): Promise<CreateVmStreamResult> {
  const res = await fetch(`${API}/vms/stream`, {
    method: 'POST',
    credentials: 'same-origin',
    headers: {
      'Content-Type': 'application/json',
      Accept: 'text/event-stream',
    },
    body: JSON.stringify(req),
  })
  if (!res.ok) throw await streamResponseError(res)
  if (!res.body) throw new Error('No response body')
  const reader = res.body.getReader()
  const dec = new TextDecoder()
  let buf = ''
  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    buf += dec.decode(value, { stream: true }).replace(/\r\n/g, '\n')
    for (;;) {
      const idx = buf.indexOf('\n\n')
      if (idx < 0) break
      const block = buf.slice(0, idx)
      buf = buf.slice(idx + 2)
      let ev = 'message'
      const dataLines: string[] = []
      for (const ln of block.split('\n')) {
        if (ln.startsWith('event:')) ev = ln.slice(6).trim()
        else if (ln.startsWith('data:')) dataLines.push(ln.slice(5).trimStart())
      }
      const data = dataLines.join('\n')
      if (ev === 'complete') {
        return JSON.parse(data) as CreateVmStreamResult
      }
      if (ev === 'error') {
        throw new Error(formatUserError(data || 'Create failed'))
      }
      if (ev === 'job') {
        try {
          const j = JSON.parse(data) as { id?: string }
          if (j?.id) onJobRegistered?.({ id: j.id })
        } catch {
          /* ignore */
        }
        continue
      }
      if (data && data !== 'keepalive') onLogLine(data)
    }
  }
  throw new Error('Stream ended before VM was created')
}
/** Optional `virDomainUndefineFlags` query params for `DELETE /vms/{name}`. */
export interface VmDeleteUndefineOpts {
  undefine_managed_save?: boolean
  undefine_snapshots_metadata?: boolean
  undefine_nvram?: boolean
  undefine_keep_nvram?: boolean
  undefine_checkpoints_metadata?: boolean
  undefine_tpm?: boolean
  undefine_keep_tpm?: boolean
  /** Also remove backing disk image files from the host filesystem. */
  delete_disks?: boolean
}

function deleteVmQuery(opts?: VmDeleteUndefineOpts): string {
  if (!opts) return ''
  const p = new URLSearchParams()
  const set = (k: keyof VmDeleteUndefineOpts) => {
    if (opts![k]) p.set(k, 'true')
  }
  set('undefine_managed_save')
  set('undefine_snapshots_metadata')
  set('undefine_nvram')
  set('undefine_keep_nvram')
  set('undefine_checkpoints_metadata')
  set('undefine_tpm')
  set('undefine_keep_tpm')
  set('delete_disks')
  const s = p.toString()
  return s ? `?${s}` : ''
}

export const deleteVM = (name: string, undefine?: VmDeleteUndefineOpts, connection?: string | null) =>
  apiDelete(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}${deleteVmQuery(undefine)}`, connection),
  )

export interface BlockJobInfo {
  job_type: number
  bandwidth: number
  cur: number
  end: number
}

export const getBlockJobInfo = (name: string, disk: string, bandwidthBytes = false, connection?: string | null) =>
  readJsonObject<{ name: string; job: BlockJobInfo | null }>(
    appendVmConnection(
      `${API}/vms/${encodeURIComponent(name)}/block/job?disk=${encodeURIComponent(disk)}${bandwidthBytes ? '&bandwidth_bytes=true' : ''}`,
      connection,
    ),
  )

export const blockCommit = (
  name: string,
  body: {
    disk: string
    base?: string | null
    top?: string | null
    bandwidth?: number
    shallow?: boolean
    delete?: boolean
    active?: boolean
    relative?: boolean
    bandwidth_bytes?: boolean
  },
  connection?: string | null,
) => apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/block/commit`, connection), body)

export const blockPull = (
  name: string,
  body: { disk: string; bandwidth?: number; bandwidth_bytes?: boolean },
  connection?: string | null,
) => apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/block/pull`, connection), body)

export const blockJobAbort = (
  name: string,
  body: { disk: string; async?: boolean; pivot?: boolean },
  connection?: string | null,
) => apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/block/job/abort`, connection), body)

export const setMemTune = (name: string, body: MemTuneInfo, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/memtune`, connection), body)

export const setSchedulerTune = (
  name: string,
  body: { cpu_shares?: number; vcpu_period?: number; vcpu_quota?: number },
  connection?: string | null,
) => apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/scheduler`, connection), body)

export const pinVcpu = (name: string, vcpu: number, cpus: boolean[], connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/vcpu/${vcpu}/pin`, connection), { cpus })
export const startVM = (name: string, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/start`, connection))
export const stopVM = (name: string, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/stop`, connection))
export const shutdownVM = (name: string, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/shutdown`, connection))
export const rebootVM = (name: string, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/reboot`, connection))
export const pauseVM = (name: string, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/pause`, connection))
export const resumeVM = (name: string, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/resume`, connection))
export const cloneVM = (
  name: string,
  newName: string,
  connection?: string | null,
  cloneMode: 'linked' | 'full' | 'xml' = 'linked',
) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/clone`, connection), {
    new_name: newName,
    clone_mode: cloneMode,
  })
export const setAutostart = (name: string, enabled: boolean, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/autostart/${enabled}`, connection))
export const setVcpus = (name: string, count: number, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/vcpus/${count}`, connection))
export const setMemory = (name: string, mb: number, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/memory/${mb}`, connection))
export const renameVM = (name: string, newName: string, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/rename`, connection), { new_name: newName })
export const getMetrics = () => readJsonArray<VmMetrics>(`${API}/metrics`)
export const getVMMetrics = (name: string, connection?: string | null) =>
  readJsonObject<VmMetrics>(appendVmConnection(`${API}/metrics/${encodeURIComponent(name)}`, connection))

interface MetricsHistoryPoint {
  timestamp_ms: number
  host_cpu_percent: number
  host_memory_percent: number
  host_disk_percent: number
  load_1: number
  vms_running: number
  vm_count: number
  vm_metrics: VmMetrics[]
}

export const getMetricsHistory = (limit = 60) =>
  readJsonObject<{ points: MetricsHistoryPoint[] }>(`${API}/metrics/history?limit=${limit}`)
export const getTemplates = () => readJsonArray<VmTemplate>(`${API}/templates`)

export interface GuestIpAddress {
  name: string
  mac: string
  ip_type: string
  address: string
  prefix: number
  /** `lease` | `arp` | `agent` — libvirt source for this row (deduped; first source wins). */
  source: string
  /** From libvirt DHCP lease table (matched by IP/MAC). */
  dhcp_hostname?: string
  /** RFC3339 when matched from `virsh net-dhcp-leases`. */
  dhcp_expires_at?: string
  /** Relative to `queried_at` / server time. */
  lease_seconds_remaining?: number
  /** Reverse DNS (PTR) from the hypervisor when resolvable. */
  dns_ptr?: string
}

interface GuestInterfacesResponse {
  addresses: GuestIpAddress[]
  /** ISO-8601 — when the hypervisor collected this snapshot. */
  queried_at: string
  /** libvirt network name → IPv4 gateway from network XML (when resolvable). */
  network_gateways?: Record<string, string>
}

export interface BootConfig {
  boot_devices: string[]
  firmware: string
  secure_boot: boolean
  kernel?: string
  initrd?: string
  cmdline?: string
}

interface ManagedSaveStatus {
  name: string
  has_managed_save: boolean
}

export const getInterfaces = (name: string, connection?: string | null) =>
  readJsonObject<GuestInterfacesResponse>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/interfaces`, connection),
  )
export const getHostname = (name: string, connection?: string | null) =>
  readJsonObject<{ hostname: string }>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/hostname`, connection),
  )

interface GuestFilesystemMetric {
  mountpoint: string
  name: string
  fs_type: string
  total_bytes: number
  used_bytes: number
}

export interface GuestObservability {
  hostname: string
  os_type: string
  os_version: string
  ip_addresses: GuestIpAddress[]
  filesystems: GuestFilesystemMetric[]
}

export const getGuestObservability = (name: string, connection?: string | null) =>
  readJsonObject<GuestObservability>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/guest-observability`, connection),
  )

export interface GuestHealthReport {
  vm_name: string
  state: string
  agent_reachable: boolean
  metrics_available: boolean
  metrics?: VmMetrics | null
  guest?: GuestObservability | null
  issues: string[]
  healthy: boolean
  os_pretty_name?: string | null
  cloud_init_status?: string | null
}

export const getGuestHealth = (name: string, connection?: string | null) =>
  readJsonObject<GuestHealthReport>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/guest-health`, connection),
  )
interface CdromInsertResult {
  status: string
  target: string
  bus: string
  /** False when the media is only staged and the guest must restart to see it. */
  live: boolean
  requires_restart: boolean
  message: string
}

/** Insert or swap CD-ROM media. Pass an empty `target` to let the daemon pick a free one. */
export const insertCdrom = (name: string, isoPath: string, target = '', connection?: string | null) =>
  apiPost<CdromInsertResult>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/cdrom/insert`, connection),
    { iso_path: isoPath, target },
  )
/** Blank the media, leaving the drive attached. */
export const ejectCdrom = (name: string, target: string, connection?: string | null) =>
  apiPostVoid(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/cdrom/eject/${encodeURIComponent(target)}`, connection),
  )
interface GuestAgentInstallResult {
  status: string
  vm: string
  iso_path: string
  iso_downloaded: boolean
  cdrom: { target: string; bus: string; live: boolean; requires_restart: boolean }
  channel: { present: boolean; added: boolean; requires_restart: boolean; added_controller: boolean } | null
  requires_restart: boolean
  next_step: string
}

/**
 * Stage the in-guest agent: fetch the agent ISO if the host lacks it, attach it
 * at a free CD-ROM target, and add the QEMU guest-agent channel when missing.
 */
export const installGuestAgentMedia = (name: string, connection?: string | null) =>
  apiPost<GuestAgentInstallResult>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/guest-agent/install-media`, connection),
    {},
  )

interface WindowsRdpEnableResult {
  status: string
  vm: string
  result: { disk_path: string; applied: string[]; firewall_manual: boolean; notes: string[] }
}

/** Enable Remote Desktop offline. Requires the VM to be powered off. */
export const enableWindowsRdp = (name: string, connection?: string | null) =>
  apiPost<WindowsRdpEnableResult>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/windows/enable-rdp`, connection),
    {},
  )

interface LinuxOfflineResult {
  status: string
  vm: string
  result: { disk_path: string; operation: string; applied: string[]; notes: string[] }
}

/** Offline Linux SSH enable (GuestKit linux-ssh plan). VM must be shut off. */
export const enableLinuxSsh = (name: string, connection?: string | null) =>
  apiPost<LinuxOfflineResult>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/linux/enable-ssh`, connection),
    {},
  )

export const injectLinuxSshKey = (
  name: string,
  body: { user: string; public_key: string },
  connection?: string | null,
) =>
  apiPost<LinuxOfflineResult>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/linux/inject-ssh-key`, connection),
    body,
  )

export const resetLinuxPassword = (
  name: string,
  body: { user: string; password: string },
  connection?: string | null,
) =>
  apiPost<LinuxOfflineResult>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/linux/reset-password`, connection),
    body,
  )

export const fixLinuxFstab = (name: string, connection?: string | null) =>
  apiPost<LinuxOfflineResult>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/linux/fix-fstab`, connection),
    {},
  )

export const setLinuxHostname = (
  name: string,
  body: { hostname: string },
  connection?: string | null,
) =>
  apiPost<LinuxOfflineResult>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/linux/set-hostname`, connection),
    body,
  )

export const addShare = (name: string, sourceDir: string, mountTag: string, xattr: boolean, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/share`, connection), {
    source_dir: sourceDir,
    mount_tag: mountTag,
    xattr,
  })

export const removeShare = (name: string, mountTag: string, connection?: string | null) =>
  apiDelete(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/share/${encodeURIComponent(mountTag)}`, connection),
  )
export const managedSave = (name: string, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/managed-save`, connection))
export const managedSaveRemove = (name: string, connection?: string | null) =>
  apiDelete(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/managed-save`, connection))
export const hasManagedSave = (name: string, connection?: string | null) =>
  readJsonObject<ManagedSaveStatus>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/managed-save/status`, connection),
  )
export const getBootConfig = (name: string, connection?: string | null) =>
  readJsonObject<BootConfig>(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/boot`, connection))
export const setBootOrder = (name: string, devices: string[], connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/boot`, connection), { devices })
interface MigrateOptions {
  parameters?: { bandwidth?: number; bandwidth_postcopy?: number; parallel_connections?: number }
  extra_flags?: number
  unsafe_migrate?: boolean
  postcopy?: boolean
  undefine_source?: boolean
  tunnelled?: boolean
  paused?: boolean
  /** FluxVM: bearer token for a remote target FluxVM API in `destUri`. */
  dest_token?: string
  listen_host?: string
  advertise_host?: string
  bandwidth_mbps?: number
  max_downtime_ms?: number
}

export const migrateVM = (name: string, destUri: string, live: boolean, opts?: MigrateOptions, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/migrate`, connection), {
    dest_uri: destUri,
    live,
    ...opts,
  })
export const setMigrateBandwidth = (name: string, mbps: number) =>
  apiPost<{ mbps: number }>(`${API}/vms/${encodeURIComponent(name)}/migrate/max-bandwidth`, { mbps })

type GuestKeyPreset = 'ctrl_alt_del' | 'esc' | 'alt_tab'

export const sendGuestKey = (
  name: string,
  body: { preset?: GuestKeyPreset; keycodes?: number[]; holdtime_ms?: number },
  connection?: string | null,
) => apiPost(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/guest/send-key`, connection), body)

export const setVmFirmware = (name: string, uefi: boolean, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/firmware`, connection), { uefi })

export const attachVmTpm = (name: string, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/devices/tpm`, connection))
export const detachVmTpm = (name: string, connection?: string | null) =>
  apiDelete(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/devices/tpm`, connection))

export const attachVmWatchdog = (name: string, model: string, action: string, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/devices/watchdog`, connection), {
    model,
    action,
  })

export const attachVmSound = (name: string, model: string, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/devices/sound`, connection), { model })

export const attachVmSerial = (name: string, port: number, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/devices/serial`, connection), { port })

export const setVmVideoModel = (name: string, model: string, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/devices/video-model`, connection), { model })

interface DiskTuneBody {
  target: string
  bus?: string
  cache?: string
  discard?: string
  readonly?: boolean
  shareable?: boolean
}

export const tuneVmDisk = (name: string, body: DiskTuneBody, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/disk/tune`, connection), body)

interface NicTuneBody {
  mac_address: string
  model?: string
  network?: string
}

export const tuneVmNic = (name: string, body: NicTuneBody, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/nic/tune`, connection), body)

/** Run `virt-xml --convert-to-vnc` for this domain (requires virt-xml on the host). */
export const convertGraphicsSpiceToVnc = (name: string, connection?: string | null) =>
  apiPost<unknown>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/graphics/convert-to-vnc`, connection),
    {},
  )

export const setMemoryBalloon = (name: string, mb: number, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/balloon/${mb}`, connection))
export const resizeDisk = (name: string, target: string, sizeGb: number, connection?: string | null) =>
  apiPostVoid(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/disk/resize/${encodeURIComponent(target)}`, connection),
    { size_gb: sizeGb },
  )
export const attachInterface = (name: string, network: string, model: string = 'virtio', connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/nic/attach`, connection), { network, model })
export const detachInterface = (name: string, mac: string, connection?: string | null) =>
  apiPostVoid(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/nic/detach/${encodeURIComponent(mac)}`, connection),
  )
export const getVMLogs = (name: string, lines = 500, connection?: string | null) =>
  readJsonObject<{ vm_name: string; log_path: string; content: string }>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/logs?lines=${lines}`, connection),
  )

export interface CpuTuneInfo { shares?: number; period?: number; quota?: number; vcpupin: { vcpu: number; cpuset: string }[] }
export interface MemTuneInfo { hard_limit_kb?: number; soft_limit_kb?: number; swap_hard_limit_kb?: number }
export const getCpuTune = (name: string, connection?: string | null) =>
  readJsonObject<CpuTuneInfo>(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/cputune`, connection))
export const getMemTune = (name: string, connection?: string | null) =>
  readJsonObject<MemTuneInfo>(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/memtune`, connection))

interface NumaTuneState {
  node_set?: string | null
  mode?: number | null
}

export const getNumaTune = (name: string, connection?: string | null) =>
  readJsonObject<NumaTuneState>(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/numa`, connection))

export const setNumaTune = (
  name: string,
  body: { node_set?: string | null; mode?: number | null },
  connection?: string | null,
) => apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/numa`, connection), body)

export const pinEmulator = (name: string, cpus: boolean[], connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/emulator/pin`, connection), { cpus })

/** Classic-mode ConsoleHub (daemon API). */
export interface ClassicConsoleHubPlan {
  vm_name: string
  recommended: string
  native: { console_type: string; ws_path: string; serial_ws_path?: string; available: boolean }
  guest_ip?: string | null
  ssh_user?: string | null
  os_hint: string
  protocols: string[]
  webrtc_spice_available: boolean
}

export interface ClassicConsoleHubSessionResponse {
  session_id: string
  vm_name: string
  protocol: string
  backend: string
  embed_path: string
  emergency_url?: string | null
  audit_id: string
  expires_at: string
  recording_enabled?: boolean
}

export const getClassicConsoleHubPlan = (name: string, connection?: string | null) =>
  readJsonObject<ClassicConsoleHubPlan>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/consolehub/plan`, connection),
  )

export const createClassicConsoleHubSession = (
  name: string,
  body: { protocol?: string },
  connection?: string | null,
) =>
  apiPost<ClassicConsoleHubSessionResponse>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/consolehub/sessions`, connection),
    body,
  )

export const listClassicConsoleHubSessions = (name: string, connection?: string | null) =>
  readJsonArray<{
    session_id: string
    vm_name: string
    actor: string
    protocol: string
    backend: string
    started_at: string
    ended_at?: string | null
  }>(appendVmConnection(`${API}/vms/${encodeURIComponent(name)}/consolehub/sessions`, connection))
