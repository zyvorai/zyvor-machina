// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { apiPost, apiPostVoid, apiDelete, readJsonArray, readJsonObject } from './client'
import { parseResponseError } from './parseResponseError'
import { VmTemplate } from './vm'

const API = '/api/v1'

export interface ImageFile {
  path: string
  name: string
  size_bytes: number
  format: string
}

/** ISO / disk browse API: discovered files plus directories scanned (libvirt pools + defaults). */
interface BrowseFilesResponse {
  files: ImageFile[]
  scan_directories: string[]
}

interface BrowseDirEntry {
  name: string
  path: string
  is_directory: boolean
  size_bytes: number
}

export interface BrowseDirResponse {
  path: string
  parent: string | null
  entries: BrowseDirEntry[]
  roots: string[]
}

export interface UsbDevice {
  bus: string
  device: string
  vendor_id: string
  product_id: string
  description: string
}

export interface AuditEvent {
  timestamp: string
  action: string
  target: string
  result: string
  /** UNIX user or token label when recorded by the daemon. */
  actor?: string
}

interface GetAuditLogParams {
  action?: string
  actor?: string
  q?: string
  limit?: number
}

// ISO/Disk browser
export const listIsos = () => readJsonObject<BrowseFilesResponse>(`${API}/browse/isos`)
/** List one directory under allowed hypervisor roots (pools + /home, /media, …). Pass empty path to start at the first root. */
export const browseDir = (path = '') =>
  readJsonObject<BrowseDirResponse>(`${API}/browse/dir?path=${encodeURIComponent(path)}`)
export const listDiskImages = () => readJsonObject<BrowseFilesResponse>(`${API}/browse/disks`)

interface IsoDownloadStarted {
  status: string
  job_id: string
  name: string
  path: string
}

/** Kick off a server-side ISO download; poll `getJob(job_id)` for progress. */
export const downloadIsoFromUrl = (body: { url: string; filename?: string; overwrite?: boolean }) =>
  apiPost<IsoDownloadStarted>(`${API}/browse/isos/download`, body)

export interface JobSummary {
  id: string
  kind: string
  title: string
  status: 'running' | 'completed' | 'failed'
  target_path?: string
  source_url?: string
  error?: string
  bytes_done?: number
  bytes_total?: number
  created_unix: number
  updated_unix: number
}

export const listJobs = () => readJsonArray<JobSummary>(`${API}/jobs`)

interface IsoUploadResult {
  status: string
  name: string
  path: string
  size_bytes: number
}

interface IsoUploadOptions {
  /** Replace an ISO of the same name that is already on the hypervisor. */
  overwrite?: boolean
  onProgress?: (pct: number, loaded: number, total: number) => void
  signal?: AbortSignal
}

/**
 * Stream an ISO from the user's machine to the hypervisor's upload directory.
 *
 * Uses XMLHttpRequest rather than `fetch` because only XHR reports upload
 * progress events, and a Windows ISO is several GiB — a progress-less spinner
 * for that long is indistinguishable from a hang.
 */
export function uploadIso(file: File, opts: IsoUploadOptions = {}): Promise<IsoUploadResult> {
  return new Promise((resolve, reject) => {
    const params = new URLSearchParams({ filename: file.name })
    if (opts.overwrite) params.set('overwrite', 'true')

    const xhr = new XMLHttpRequest()
    xhr.open('POST', `${API}/browse/isos/upload?${params.toString()}`)
    xhr.withCredentials = true
    xhr.setRequestHeader('Content-Type', 'application/octet-stream')

    xhr.upload.onprogress = (e) => {
      if (!e.lengthComputable || !opts.onProgress) return
      opts.onProgress(Math.round((e.loaded / e.total) * 100), e.loaded, e.total)
    }

    xhr.onload = () => {
      let parsed: { error?: string; status?: string } | null = null
      try {
        parsed = JSON.parse(xhr.responseText)
      } catch {
        // A proxy error page rather than the daemon's JSON.
        parsed = null
      }
      if (xhr.status >= 200 && xhr.status < 300 && parsed) {
        resolve(parsed as unknown as IsoUploadResult)
        return
      }
      reject(new Error(parsed?.error || `Upload failed (HTTP ${xhr.status})`))
    }
    xhr.onerror = () => reject(new Error('Upload failed — the connection was lost'))
    xhr.onabort = () => reject(new Error('Upload cancelled'))

    if (opts.signal) {
      if (opts.signal.aborted) {
        reject(new Error('Upload cancelled'))
        return
      }
      opts.signal.addEventListener('abort', () => xhr.abort(), { once: true })
    }

    xhr.send(file)
  })
}
export const deleteDiskImage = (path: string) =>
  apiDelete(`${API}/browse/disks/delete?path=${encodeURIComponent(path)}`)

interface VirtBuilderTemplateRow {
  name: string
  summary?: string | null
  arch?: string | null
  size?: string | null
}

/** Response from `virt-builder --list --list-format json` (with plain-list fallback). Cached ~5 minutes on the daemon unless `refresh`. */
export interface VirtBuilderListResponse {
  /** When false, virt-builder APIs are disabled in daemon config. */
  virt_builder_allowed?: boolean
  /** Whether `virt-builder --version` succeeds on the host. */
  virt_builder_installed?: boolean
  /** First line of `virt-builder --version` (diagnostics). */
  virt_builder_version?: string | null
  /** Populated when the catalog could not be loaded (or feature is off / binary missing). */
  catalog_error?: string | null
  format_version: number
  items: VirtBuilderTemplateRow[]
  templates: string[]
  cached?: boolean
  cache_age_secs?: number | null
  source_uri?: string | null
}

/** Allowed absolute path prefixes for virt-image-build / new qcow2 output (pool targets + defaults). */
export const getVirtImageOutputRoots = () =>
  readJsonObject<{ allowed_prefixes: string[]; effective_tmpdir: string }>(
    `${API}/browse/virt-image-output-roots`,
  )

export const listVirtBuilderTemplates = (opts?: { refresh?: boolean }) => {
  const q = opts?.refresh ? '?refresh=true' : ''
  return readJsonObject<VirtBuilderListResponse>(`${API}/browse/virt-builder${q}`)
}

/** Lightweight check: name format + optional presence in server catalog cache. */
export const probeVirtBuilderTemplate = (template: string) =>
  readJsonObject<{
    virt_builder_allowed: boolean
    name_valid: boolean
    in_cached_catalog: boolean
    hint?: string | null
  }>(`${API}/browse/virt-builder/probe/${encodeURIComponent(template.trim())}`)

/** Request body for POST /browse/virt-image-build (daemon runs `virt-image-build` / `virt-builder` on the host). */
export interface VirtImageBuildRequest {
  os: string
  /** Absolute path; file must not exist; parent under allowed image dirs. */
  output: string
  size?: string
  format?: string
  hostname?: string
  install?: string
  run_command?: string[]
  copy_in?: string[]
  firstboot_script?: string | null
  root_password_file?: string | null
  root_password_inline?: string | null
  ssh_pubkey_file?: string | null
  ssh_pubkey_inline?: string | null
  update?: boolean
  selinux_relabel?: boolean
  extra_virt_builder_args?: string[]
}

export const buildVirtImageDisk = (body: VirtImageBuildRequest) =>
  apiPost<{ status: string; path: string }>(`${API}/browse/virt-image-build`, body)

export const getVirtBuilderNotes = (template: string) =>
  readJsonObject<{ template: string; notes: string }>(
    `${API}/browse/virt-builder/notes/${encodeURIComponent(template)}`,
  )

/** mkosi workspace directories discovered under /var/lib/machina/mkosi-defs/ etc. */
export interface MkosiWorkspace {
  path: string
  name: string
  /** Sub-images in an mkosi image tree (subdirs containing mkosi.conf). */
  images: string[]
}

export const listMkosiWorkspaces = () => readJsonArray<MkosiWorkspace>(`${API}/browse/mkosi-workspaces`)

// USB
export const listUsbDevices = () => readJsonArray<UsbDevice>(`${API}/host/usb`)
export const attachUsb = (vm: string, vendor_id: string, product_id: string) =>
  apiPostVoid(`${API}/vms/${encodeURIComponent(vm)}/usb/attach`, { vendor_id, product_id })
export const detachUsb = (vm: string, vendor_id: string, product_id: string) =>
  apiPostVoid(`${API}/vms/${encodeURIComponent(vm)}/usb/detach`, { vendor_id, product_id })

// Cloud-init
export const generateCloudInit = (hostname: string, username: string, password: string, ssh_key: string) =>
  apiPost<{ status: string; path: string }>(`${API}/cloud-init`, { hostname, username, password, ssh_key })

// Import
export const importDisk = (source: string, dest_name: string) =>
  apiPost<{ status: string; path: string }>(`${API}/import/disk`, { source, dest_name })

// Live resize
export const liveSetVcpus = (vm: string, count: number) => apiPostVoid(`${API}/vms/${encodeURIComponent(vm)}/live/vcpus/${count}`)
export const liveSetMemory = (vm: string, mb: number) => apiPostVoid(`${API}/vms/${encodeURIComponent(vm)}/live/memory/${mb}`)

// Audit
export const getAuditLog = (params?: GetAuditLogParams) => {
  const sp = new URLSearchParams()
  if (params?.action?.trim()) sp.set('action', params.action.trim())
  if (params?.actor?.trim()) sp.set('actor', params.actor.trim())
  if (params?.q?.trim()) sp.set('q', params.q.trim())
  if (params?.limit != null) sp.set('limit', String(params.limit))
  const qs = sp.toString()
  return readJsonArray<AuditEvent>(`${API}/audit${qs ? `?${qs}` : ''}`)
}

/** Download full audit log as NDJSON for SIEM (requires session cookie). */
export async function exportAuditNdjson(): Promise<void> {
  const res = await fetch(`${API}/audit/export`, { credentials: 'same-origin' })
  if (!res.ok) throw await parseResponseError(res)
  const blob = await res.blob()
  const a = document.createElement('a')
  a.href = URL.createObjectURL(blob)
  a.download = `machina-audit-${new Date().toISOString().slice(0, 10)}.ndjson`
  a.click()
  URL.revokeObjectURL(a.href)
}

// Tags
export const getVmTags = async (vm: string): Promise<{ tags: string[] }> => {
  const o = await readJsonObject<{ tags?: unknown }>(`${API}/vms/${encodeURIComponent(vm)}/tags`)
  const tags = Array.isArray(o.tags)
    ? (o.tags as unknown[]).filter((x): x is string => typeof x === 'string')
    : []
  return { tags }
}
export const setVmTags = (vm: string, tags: string[]) =>
  apiPost<{ status: string; name: string; tags: string[] }>(`${API}/vms/${encodeURIComponent(vm)}/tags`, { tags })
export const getAllTags = async (): Promise<Record<string, number>> => {
  const raw = await readJsonObject<Record<string, unknown>>(`${API}/tags`)
  const out: Record<string, number> = {}
  for (const [k, v] of Object.entries(raw)) {
    if (typeof v === 'number' && Number.isFinite(v)) out[k] = v
  }
  return out
}

// DHCP leases
export interface DhcpLease {
  network: string
  mac: string
  ip: string
  hostname: string
  expiry: string
}
export const listDhcpLeases = () => readJsonArray<DhcpLease>(`${API}/dhcp-leases`)

// Host stats
export interface HostStats {
  cpu_percent: number
  memory_total_mb: number
  memory_used_mb: number
  memory_percent: number
  swap_total_mb: number
  swap_used_mb: number
  disk_total_gb: number
  disk_used_gb: number
  disk_percent: number
  load_1: number
  load_5: number
  load_15: number
  uptime_secs: number
  processes: number
}
export const getHostStats = () => readJsonObject<HostStats>(`${API}/host/stats`)

interface PressureAvg {
  some: number
  full: number
  total: number
}

interface HostPressureStats {
  cpu: PressureAvg
  memory: PressureAvg
  io: PressureAvg
  available: boolean
}

interface DiskIoStat {
  device: string
  read_bytes: number
  write_bytes: number
  read_ios: number
  write_ios: number
}

interface SmartDiskHealth {
  device: string
  passed: boolean
  summary: string
  probed: boolean
}

interface CgroupV2Stats {
  unified_path: string
  memory_current_bytes?: number | null
  memory_max_bytes?: number | null
  cpu_usage_usec?: number | null
  available: boolean
}

export interface VmCgroupStats {
  vm_name: string
  cgroup_path: string
  memory_current_bytes?: number | null
  memory_max_bytes?: number | null
  cpu_usage_usec?: number | null
  available: boolean
}

interface HwmonTemp {
  sensor: string
  label: string
  temp_celsius: number
  critical_celsius?: number | null
}

interface BpfProbeSummary {
  available: boolean
  bpftool_path: string
  program_count: number
  map_count: number
  cgroup_program_count: number
  tracepoint_count: number
  notes: string[]
}

export interface LinuxHostObservability {
  pressure: HostPressureStats
  disk_io: DiskIoStat[]
  smart: SmartDiskHealth[]
  cgroup: CgroupV2Stats
  thermal?: HwmonTemp[]
  vm_cgroups?: VmCgroupStats[]
  bpf: BpfProbeSummary
}

export const getHostLinuxObservability = () =>
  readJsonObject<LinuxHostObservability>(`${API}/host/linux-observability`)

interface LinuxAuditEvent {
  timestamp: string
  event_type: string
  summary: string
  raw: string
}

export interface LinuxAuditReport {
  available: boolean
  source: string
  events: LinuxAuditEvent[]
  avc_count: number
}

export const getHostLinuxAudit = () =>
  readJsonObject<LinuxAuditReport>(`${API}/host/linux-audit`)

/** Per-mount usage from `df` (Linux hypervisor). */
export interface HostFilesystem {
  source: string
  fstype: string
  mount_point: string
  size_bytes: number
  used_bytes: number
  avail_bytes: number
  use_percent: number
}

export const getHostFilesystems = () => readJsonArray<HostFilesystem>(`${API}/host/filesystems`)

/** Top processes by resident memory (Linux `ps`). */
export interface HostProcess {
  pid: number
  user: string
  cpu_percent: number
  rss_kb: number
  command: string
  /** Full argv from `/proc/pid/cmdline` when present (Linux). */
  args?: string
}

/** `sort`: `rss` (default) = highest memory first; `cpu` = highest %CPU first. */
type HostTopProcessSort = 'rss' | 'cpu'

export const getHostTopProcesses = (limit = 20, opts?: { sort?: HostTopProcessSort }) => {
  const sp = new URLSearchParams()
  sp.set('limit', String(limit))
  if (opts?.sort && opts.sort !== 'rss') sp.set('sort', opts.sort)
  return readJsonArray<HostProcess>(`${API}/host/processes?${sp.toString()}`)
}

/** Send SIGTERM or SIGKILL to a process on the hypervisor (admin browser session only). */
export const postHostKillProcess = (pid: number, opts?: { signal?: 'TERM' | 'KILL' }) =>
  apiPost<{ ok: boolean; pid: number; signal: string }>(`${API}/host/processes/kill`, {
    pid,
    ...(opts?.signal ? { signal: opts.signal } : {}),
  })

/** Distro-specific read-only update probe (apt/dnf/yum/pacman/zypper). */
export interface PackageUpdateCheck {
  backend: string
  probed: boolean
  pending_count: number | null
  summary: string | null
  hint: string | null
  error: string | null
  /** Linux: true when `/var/run/reboot-required` exists (common on Debian/Ubuntu after certain upgrades). */
  reboot_required?: boolean
}

export const getHostPackageUpdates = () => readJsonObject<PackageUpdateCheck>(`${API}/host/package-updates`)

/** Result of `package-upgrade`, `package-install`, or `package-remove` (stdout/stderr from the distro tool). */
export interface PackageActionResult {
  command: string
  exit_code: number
  stdout: string
  stderr: string
  ok: boolean
}

/** Upgrade all pending packages (distro-specific; browser session only). Set `dry_run` for a no-change preview when supported. */
export const postHostPackageUpgrade = (opts?: { dry_run?: boolean }) =>
  apiPost<PackageActionResult>(`${API}/host/package-upgrade`, { dry_run: opts?.dry_run === true })

export const postHostPackageInstall = (packages: string[]) =>
  apiPost<PackageActionResult>(`${API}/host/package-install`, { packages })

/** apt: runs `apt-get autoremove` (browser session only). */
export const postHostPackageAutoremove = () =>
  apiPost<PackageActionResult>(`${API}/host/package-autoremove`, {})

export const postHostPackageRemove = (packages: string[], opts?: { purge?: boolean }) =>
  apiPost<PackageActionResult>(`${API}/host/package-remove`, {
    packages,
    ...(opts?.purge ? { purge: true } : {}),
  })

export interface NetDevCounter {
  iface: string
  rx_bytes: number
  rx_packets: number
  tx_bytes: number
  tx_packets: number
}

export const getHostNetCounters = () => readJsonArray<NetDevCounter>(`${API}/host/net-counters`)

interface NetDevRate {
  iface: string
  rx_bytes_per_sec: number
  tx_bytes_per_sec: number
  rx_packets_per_sec: number
  tx_packets_per_sec: number
}

export interface NetDevRatesResponse {
  sample_interval_ms: number
  interfaces: NetDevRate[]
}

/** Two `/proc/net/dev` samples; blocks ~interval_ms on the server. */
export const getHostNetRates = (intervalMs = 1000) =>
  readJsonObject<NetDevRatesResponse>(
    `${API}/host/net-rates?interval_ms=${encodeURIComponent(String(Math.min(5000, Math.max(50, intervalMs))))}`,
  )

export interface PasswdEntry {
  username: string
  uid: number
  gid: number
  gecos: string
  home: string
  shell: string
  system_account: boolean
}

export const getHostPasswdUsers = (limit = 150) =>
  readJsonArray<PasswdEntry>(`${API}/host/passwd-users?limit=${encodeURIComponent(String(limit))}`)

export interface GroupEntry {
  name: string
  gid: number
  members: string[]
}

export const getHostGroups = (limit = 150) =>
  readJsonArray<GroupEntry>(`${API}/host/groups?limit=${encodeURIComponent(String(limit))}`)

export interface HostSecuritySummary {
  network_backend: string
  firewall_backend: string
  ufw_status_line: string | null
  firewalld_default_zone: string | null
  selinux_mode?: string | null
}

export const getHostSecuritySummary = () => readJsonObject<HostSecuritySummary>(`${API}/host/security-summary`)

// Save as template
export const saveVmAsTemplate = (vm: string, templateName: string) =>
  apiPost<{ status: string }>(`${API}/vms/${encodeURIComponent(vm)}/save-template`, { template_name: templateName })

// List saved templates
export const listSavedTemplates = () => readJsonArray<VmTemplate>(`${API}/templates/saved`)

// PCI
export interface PciDevice {
  slot: string
  class: string
  vendor: string
  device: string
  iommu_group: string
}
export const listPciDevices = () => readJsonArray<PciDevice>(`${API}/host/pci`)

// IOMMU Groups
interface IommuDevice {
  bdf: string
  vendor: string
  device_name: string
}
export interface IommuGroup {
  group_id: number
  devices: IommuDevice[]
}
export const listIommuGroups = () => readJsonArray<IommuGroup>(`${API}/host/iommu-groups`)

// Systemd Services
export interface SystemdService {
  name: string
  description: string
  active_state: string
  sub_state: string
  enabled: string
}
export const listServices = () => readJsonArray<SystemdService>(`${API}/services`)
export const serviceAction = (name: string, action: string) =>
  apiPost<{ status: string }>(`${API}/services/${encodeURIComponent(name)}/${encodeURIComponent(action)}`)

// System Logs
export interface JournalEntry {
  timestamp: string
  unit: string
  priority: string
  message: string
}

export interface JournalBootEntry {
  index: number
  boot_id: string
  first_entry: string
  last_entry: string
}

interface JournalLogsQuery {
  lines?: number
  priority?: string
  unit?: string
  boot?: number
  since?: string
  until?: string
  grep?: string
  uid?: number
  pid?: number
  kernel?: boolean
}

export const getJournalLogs = (query: JournalLogsQuery = {}) => {
  const { lines = 100, priority, unit, boot, since, until, grep, uid, pid, kernel } = query
  const params = new URLSearchParams({ lines: String(lines) })
  if (priority) params.set('priority', priority)
  if (unit) params.set('unit', unit)
  if (boot != null && Number.isFinite(boot)) params.set('boot', String(boot))
  if (since) params.set('since', since)
  if (until) params.set('until', until)
  if (grep) params.set('grep', grep)
  if (uid != null && Number.isFinite(uid)) params.set('uid', String(uid))
  if (pid != null && Number.isFinite(pid)) params.set('pid', String(pid))
  if (kernel) params.set('kernel', 'true')
  return readJsonArray<JournalEntry>(`${API}/logs?${params.toString()}`)
}

export const getJournalBoots = () => readJsonArray<JournalBootEntry>(`${API}/logs/boots`)

// Host Shutdown/Reboot
export const hostShutdown = () => apiPost<{ status: string }>(`${API}/host/shutdown`)
export const hostReboot = () => apiPost<{ status: string }>(`${API}/host/reboot`)

// System Info
export interface SystemInfo {
  hostname: string
  timezone: string
  kernel_version: string
  architecture: string
  os_name: string
  os_version: string
  os_pretty_name: string
  boot_time: string
  rtc_time: string
  ntp_service: string
  system_clock_synchronized: boolean
  systemd_version: string
  boot_duration: string
  critical_chain_top: string[]
  logged_in_users: number
  pretty_hostname: string
  transient_hostname: string
  icon_name: string
  chassis: string
  deployment: string
  location: string
  machine_id: string
  boot_id: string
  hardware_model: string
  firmware_version: string
  local_time: string
  universal_time: string
  rtc_in_local_tz: string
  systemd_version_full: string
  systemd_analyze_blame_top: string[]
  loginctl_users_text: string
  loginctl_sessions_text: string
  systemctl_show_manager: string
  mount_units_text: string
  failed_units_text: string
  networkd_dependencies_text: string
  hostnamectl_status_text: string
  timedatectl_status_text: string
  timedatectl_show_text: string
  systemctl_is_system_running: string
  systemctl_show_environment_text: string
  systemctl_list_sockets_text: string
  systemctl_list_timers_text: string
  systemctl_list_jobs_text: string
  systemctl_status_networkd_text: string
  systemctl_status_resolved_text: string
  resolved_dependencies_text: string
  resolvectl_status_text: string
  resolvectl_statistics_text: string
  bootctl_status_text: string
  enabled_service_unit_files_text: string
  running_service_units_text: string
  loginctl_list_seats_text: string
  journalctl_list_boots_text: string
  systemd_analyze_verify_text: string
  default_target_dependencies_text: string
  product_name: string
  sys_vendor: string
  bios_version: string
  bios_date: string
  board_name: string
  serial_number: string
  cpu_model: string
  virtualization: string
}
export const getSystemInfo = () => readJsonObject<SystemInfo>(`${API}/host/system-info`)
export const setHostname = (hostname: string) => apiPost<{ status: string }>(`${API}/host/hostname`, { hostname })
export const setTimezone = (timezone: string) => apiPost<{ status: string }>(`${API}/host/timezone`, { timezone })

/** Sysfs + SMBIOS/DMI + libvirt caps — VMware-style platform inventory for audits. */
interface DmiInventory {
  product_uuid: string | null
  product_serial: string
  sys_vendor: string
  product_name: string
  board_vendor: string
  board_name: string
  bios_version: string
  bios_date: string
}

interface CpuTopologySysfs {
  logical_cpus: number
  sockets: number
  socket_package_ids: number[]
  physical_cores: number
  threads_per_core_max: number
  cores_per_socket: number[]
}

interface NumaNodeInventory {
  node_id: number
  cpu_list: string
  memory_total_kb: number
}

interface LibvirtCpuCapsule {
  cpu_model: string
  cpu_sockets: number
  cpu_cores: number
  cpu_threads: number
  numa_nodes: number
  memory_mb: number
}

export interface HardwareInventoryReport {
  collected_at_rfc3339: string
  sources: string[]
  dmi: DmiInventory
  cpu_topology: CpuTopologySysfs
  numa_nodes: NumaNodeInventory[]
  cpuinfo_vendor_id: string | null
  cpuinfo_model_name: string | null
  libvirt: LibvirtCpuCapsule | null
  libvirt_logical_cpus_derived: number | null
  consistency_notes: string[]
}

export const getHardwareInventory = () =>
  readJsonObject<HardwareInventoryReport>(`${API}/host/hardware-inventory`)

export interface HardwareInventoryHistoryResponse {
  path: string
  entries: HardwareInventoryReport[]
}

export const getHardwareInventoryHistory = (limit = 80) =>
  readJsonObject<HardwareInventoryHistoryResponse>(
    `${API}/host/hardware-inventory/history?limit=${encodeURIComponent(String(limit))}`,
  )
