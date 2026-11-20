// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { Page } from '@playwright/test'

export const platformInfo = {
  version: '0.1.0-test',
  tls: { enabled: false },
  auth: { pam_service: 'sshd', oidc_enabled: false },
  control_plane: {
    proxy_url: '/api/v1/platform/controller',
    direct_url: 'http://127.0.0.1:5093',
  },
  kubevirt: { exec_enabled: true },
  guestkit: { enabled: true, base_url: 'http://127.0.0.1:8790', insecure_tls: true },
  fleet: { enabled: false, peer_count: 0 },
}

const fleetFinder = {
  summary: '2 VM(s) · 1 running',
  smart_folders: [
    { id: 'all', label: 'All VMs', count: 2, icon: 'all' },
    { id: 'running', label: 'Running', count: 1, icon: 'running' },
  ],
  tags: [],
  projects: [],
}

const fleetMission = {
  sites: [
    {
      name: 'DC-1',
      racks: [
        {
          name: 'Rack A',
          hosts: [
            {
              id: 'h1',
              hostname: 'host-1',
              address: '127.0.0.1',
              state: 'online',
              maintenance_mode: false,
              vm_count: 1,
              cpu_percent: 35,
              memory_used_mib: 4096,
              memory_total_mib: 16384,
              site: 'DC-1',
              rack: 'Rack A',
              rack_u: 10,
            },
          ],
        },
      ],
    },
  ],
  unassigned_hosts: [],
  summary: { hosts: 1, vms: 1, hosts_online: 1, health_pct: 100 },
}

const fleetMaintenanceMission = {
  summary: '1 host(s) with updates · 1 in maintenance · 0 pending schedule(s)',
  hosts_with_updates: 1,
  hosts_in_maintenance: 1,
  pending_schedules: 0,
  hosts: [
    {
      host_id: 'h1',
      hostname: 'host-1',
      state: 'online',
      maintenance_mode: true,
      validation_status: 'ok',
      pending_packages: 3,
      reboot_required: true,
      agent_drift: false,
      update_summary: '3 packages would be upgraded',
      recommended_step: 'apply_preview',
      steps: [
        { id: 'scan', label: 'Scan fleet', status: 'done', detail: null },
        { id: 'assess', label: 'Assess risk', status: 'done', detail: '3 packages would be upgraded' },
        { id: 'schedule', label: 'Schedule window', status: 'done', detail: null },
        { id: 'enter_maintenance', label: 'Enter maintenance', status: 'done', detail: null },
        { id: 'evacuate', label: 'Evacuate VMs', status: 'skipped', detail: null },
        { id: 'apply_preview', label: 'Apply preview', status: 'ready', detail: 'Preview or apply via platform' },
        { id: 'verify_exit', label: 'Verify & exit', status: 'pending', detail: null },
      ],
      blockers: [],
    },
  ],
}

const fleetDna = {
  score: 88,
  grade: 'B',
  summary: 'Infrastructure DNA 88/100 (grade B)',
  pillars: [
    { id: 'availability', label: 'Availability', score: 100, detail: '100% fleet health · 1 hosts online' },
    { id: 'patch_hygiene', label: 'Patch hygiene', score: 0, detail: '1 of 1 hosts need OS updates' },
    { id: 'linux_health', label: 'Linux health', score: 100, detail: 'Linux health OK' },
    { id: 'backup_posture', label: 'Storage posture', score: 95, detail: 'Storage OK' },
    { id: 'compliance', label: 'Compliance', score: 82, detail: 'Fleet ops grade B+' },
  ],
}

const jarvisLanding = {
  intents: [
    { id: 'jarvis-mission-control', label: 'Mission Control', review: 'Infrastructure Earth globe', action: 'navigate', navigate: '/platform?mission=1' },
    { id: 'jarvis-maintenance-mission', label: 'Maintenance Mission', review: 'Patch timeline', action: 'navigate', navigate: '/platform/maintenance?tab=mission' },
    { id: 'jarvis-machine-finder', label: 'Machine Finder', review: 'Geography', action: 'navigate', navigate: '/platform/vms?lens=topology' },
    { id: 'jarvis-enterprise', label: 'Enterprise Keychain', review: 'API keys and air-gap bundle inventory', action: 'navigate', navigate: '/platform/enterprise?tab=keychain' },
  ],
  search_hits: [],
}

const enterpriseSecurity = {
  air_gap_bundles: 0,
  tenant_policies: 1,
  fips_profiles: 2,
  summary: '1 tenant policy · 2 FIPS profiles · 0 air-gap bundles',
}

const enterpriseFips = {
  summary: 'OpenSSL profiles',
  openssl_version: '3.0',
  profiles: [{ id: 'default', name: 'Default', tls_min_version: '1.2', fips_mode: 'off', cipher_suites: 'TLS_AES_256', notes: 'Lab profile' }],
}

const enterpriseTenants = {
  summary: '1 workspace',
  projects: [{ project_name: 'default', vm_count: 2, max_vms: 50, network_isolation: 'shared', enforce_quotas: true, quota_status: 'ok' }],
}

const platformTasks = [
  {
    id: 'task-migrate-abc123def456',
    operation: 'vm.migrate',
    status: 'running',
    progress: 45,
    message: 'Copying disk 2/4',
    created_at: new Date().toISOString(),
  },
  {
    id: 'task-backup-failed-001',
    operation: 'vm.backup',
    status: 'failed',
    progress: 72,
    message: 'NFS mount timeout',
    created_at: new Date(Date.now() - 3600_000).toISOString(),
  },
]

const platformEvents = [
  { id: 'pev-1', kind: 'host.sync', message: 'Host h1 inventory sync completed', created_at: new Date().toISOString() },
  { id: 'pev-2', kind: 'task.completed', message: 'Backup task finished for vm-1', created_at: new Date(Date.now() - 120_000).toISOString() },
]

const fleetKeychain = {
  summary: '1 credential entry',
  api_keys: 1,
  air_gap_bundles: 0,
  entries: [
    { kind: 'api_key', id: 'k1', name: 'automation', summary: 'Platform API key', status: 'active' },
  ],
}

const fleetUpdates = {
  summary: '1 host with pending updates',
  recommended_agent: '0.1.0-test',
  hosts_scanned: 1,
  hosts_with_updates: 1,
  hosts_reboot_required: 1,
  agent_drift_count: 0,
  total_pending_packages: 3,
  hosts: [
    {
      host_id: 'h1',
      hostname: 'host-1',
      agent_version: '0.1.0-test',
      agent_update_available: false,
      backend: 'apt',
      pending_count: 3,
      summary: '3 packages would be upgraded',
      reboot_required: true,
      status: 'updates',
      packages: [
        { name: 'libc6', current: '2.35', available: '2.36', security: true },
        { name: 'openssl', current: '3.0.2', available: '3.0.3', security: true },
      ],
    },
  ],
}

const fleetActivity = {
  summary: '1 running VM(s) · 1 host(s) · 0 under Linux pressure',
  running_vms: 1,
  pressure_hosts: 0,
  top_vms: [
    {
      vm_id: 'v1',
      vm_name: 'vm-1',
      host_id: 'h1',
      observed_state: 'running',
      cpu_percent: 12,
      memory_used_mib: 2048,
      memory_mib: 4096,
    },
  ],
  hosts: [
    {
      host_id: 'h1',
      hostname: 'host-1',
      state: 'online',
      cpu_percent: 35,
      memory_percent: 40,
      vm_count: 2,
      io_pressure_pct: 3,
      cpu_pressure_pct: 2,
      memory_pressure_pct: 1,
      thermal_max_c: 0,
      status: 'ok',
    },
  ],
}

const fleetGpu = {
  summary: '1 GPU host(s) · 1 GPU VM(s) · 1 CUDA-ready',
  gpu_host_count: 1,
  gpu_vm_count: 1,
  cuda_ready_hosts: 1,
  mig_hosts: 0,
  vgpu_hosts: 0,
  hosts: [
    {
      host_id: 'h1',
      hostname: 'host-1',
      site: 'DC-1',
      rack: 'Rack A',
      state: 'online',
      gpu_capable: true,
      profile: 'cuda',
      model_hint: 'NVIDIA L40',
      vm_count: 1,
      gpu_vm_count: 1,
      vgpu_slices: 0,
      cuda_ready: true,
    },
  ],
  vms: [
    {
      vm_id: 'v1',
      vm_name: 'vm-1',
      host_id: 'h1',
      hostname: 'host-1',
      observed_state: 'running',
      profile: 'cuda',
      tags: ['gpu', 'inference'],
    },
  ],
  profiles: [{ kind: 'cuda', label: 'CUDA ready', host_count: 1, vm_count: 1 }],
}

const opsRunbooks = [
  {
    id: 'rb-1',
    incident: 'host-offline',
    title: 'Host offline recovery',
    category: 'infra',
    severity: 'high',
    auto_trigger: 'host.offline',
  },
]

const opsShowback = {
  summary: 'Project showback rollup',
  total_cost_usd: 850,
  fleet_grade: 'B+',
  lines: [{ project_name: 'default', cost_usd: 850, compliance_grade: 'B+', vm_count: 1 }],
}

const fleetDesktop = {
  hosts_online: 1,
  hosts_total: 1,
  active_tasks: 0,
  slo_breach_count: 0,
  slo_count: 1,
  pressure_hosts: 0,
  zeus_status: 'idle',
  unread_notifications: 0,
  linux_summary: 'OK',
}

const sampleHost = {
  id: 'h1',
  hostname: 'host-1',
  address: '127.0.0.1',
  state: 'online',
  maintenance_mode: true,
  vm_count: 2,
  last_heartbeat_at: new Date().toISOString(),
}

const sampleHostDetail = {
  ...sampleHost,
  agent_console_addr: '127.0.0.1:8788',
  libvirt_uri: 'qemu+ssh://root@127.0.0.1/system',
  agent_version: '0.1.0-test',
  cpu_model: 'Intel Xeon',
  libvirt_version: '10.0.0',
  qemu_version: '8.2.0',
  notes: '',
  site: 'DC-1',
  rack: 'Rack A',
  rack_u: 10,
  cpu_percent: 35,
  memory_used_mib: 4096,
  memory_total_mib: 16384,
  tags: ['gpu', 'edge'],
}

const hostGpus = {
  devices: [
    {
      pci_address: '0000:01:00.0',
      vendor: 'NVIDIA',
      device_name: 'NVIDIA L40',
      iommu_group: 14,
      mig_profile: '',
    },
  ],
  nvidia_smi_summary: 'NVIDIA L40 · 46068 MiB',
}

const guestkitMigratePlan = {
  image_path: '/var/lib/libvirt/images/vm-1.qcow2',
  target: 'kvm',
  migration_score: 82,
  boot_score: 88,
  estimated_downtime_minutes: 5,
  driver_injections: [],
  required_changes: ['Verify virtio-scsi drivers post-migration'],
  licensing_warnings: [],
  summary: 'KVM migration feasible with minor driver checks',
}

const staleHost = {
  id: 'h-stale',
  hostname: 'stale-host',
  address: '127.0.0.2',
  state: 'offline',
  maintenance_mode: false,
  vm_count: 0,
  last_heartbeat_at: new Date(Date.now() - 5 * 60_000).toISOString(),
}

const sampleTemplate = {
  id: 'tpl-1',
  name: 'ubuntu-24.04',
  version: '1',
  source_disk: '/var/lib/libvirt/images/ubuntu.qcow2',
  cloud_init: true,
  os_family: 'linux',
  category: 'Ubuntu',
  description: 'Ubuntu 24.04 LTS golden image',
  featured: true,
  marketplace: true,
  icon: 'ubuntu',
  auto_fetch: true,
}

const libvirtTemplateFixture = {
  name: 'debian-12-small',
  description: 'Debian 12 minimal libvirt template',
  vcpus: 2,
  memory_mb: 2048,
  disk_gb: 20,
  os_variant: 'debian12',
}

const fleetOnlyTemplate = {
  id: 'tpl-fleet',
  name: 'rhel-9',
  version: '1.0.0',
  source_disk: '/var/lib/libvirt/images/rhel-9.qcow2',
  cloud_init: true,
  os_family: 'linux',
  category: 'Linux',
  description: 'Private fleet RHEL 9 image',
  featured: false,
  marketplace: false,
  icon: 'rhel',
  auto_fetch: false,
}

const sampleNetwork = {
  id: 'n1',
  name: 'default',
  bridge: 'virbr0',
  backend: 'bridge',
  vlan_id: null as number | null,
  segment_id: null as string | null,
}

let platformNetworks = [{ ...sampleNetwork }]

let alertRules: Array<{
  id: string
  name: string
  metric: string
  comparator: string
  threshold: number
  severity: string
  scope_project: string
  scope_tag: string
  cooldown_minutes: number
  enabled: boolean
  last_fired_at: string | null
  created_at: string
}> = [
  {
    id: 'ar1', name: 'high-cpu', metric: 'cpu_percent', comparator: 'gt', threshold: 85,
    severity: 'warning', scope_project: '', scope_tag: '', cooldown_minutes: 30, enabled: true,
    last_fired_at: null, created_at: new Date().toISOString(),
  },
]

let scheduledJobs: Array<{
  id: string
  name: string
  operation: string
  payload: string
  target_host_id: string | null
  interval_minutes: number
  enabled: boolean
  last_run_at: string | null
  created_at: string
}> = [
  {
    id: 'sj1', name: 'host-inventory-refresh', operation: 'host.inventory', payload: '{}',
    target_host_id: null, interval_minutes: 60, enabled: true, last_run_at: null,
    created_at: new Date().toISOString(),
  },
]

const mockHostCockpitStorage = {
  probed: true,
  mdraid: [{ name: '/dev/md0', detail: 'raid1 · 2 devices', state: 'clean' }],
  luks: [{ name: '/dev/mapper/luks-root', detail: 'crypt', state: 'active' }],
  lvm: [{ name: 'vg0/lv_root', detail: 'ext4 · 40G', state: 'active' }],
  stratis: [],
  vdo: [],
  multipath: [{ name: 'mpatha', detail: 'active ready running', state: 'active' }],
  iscsi: [],
  summary: 'RAID · LUKS · LVM · multipath detected',
}

const mockHostCockpitNetwork = {
  probed: true,
  connections: [{ name: 'System eth0', uuid: 'u1', kind: '802-3-ethernet', device: 'eth0', state: 'activated' }],
  bonds: [{ name: 'bond0', uuid: 'u2', kind: 'bond', device: 'bond0', state: 'activated' }],
  teams: [],
  bridges: [{ name: 'br0', uuid: 'u3', kind: 'bridge', device: 'br0', state: 'activated' }],
  vlans: [{ name: 'eth0.100', uuid: 'u4', kind: 'vlan', device: 'eth0.100', state: 'activated' }],
  wifi: [],
  wireguard: [],
  firewalld: {
    available: true,
    running: true,
    default_zone: 'public',
    zones: [{ name: 'public', target: 'default', services: ['ssh', 'dhcpv6-client'], ports: ['8080/tcp'] }],
  },
  ovs: {
    probed: true,
    available: true,
    bridges: [{ name: 'br-int', ports: ['eth0', 'patch-tun'] }],
    summary: '1 OVS bridge',
  },
  summary: '1 bond · 1 bridge · OVS · firewalld active',
}

const mockHostCockpitSystem = {
  probed: true,
  kdump: { available: true, active: true, summary: 'kdump.service active' },
  selinux: { available: true, mode: 'Enforcing', enforce_supported: true },
  tuned: { available: true, active_profile: 'virtual-guest', recommended_profile: 'virtual-guest', profiles: ['virtual-guest', 'throughput-performance'] },
  realmd: { available: true, active: false, summary: 'not joined' },
  systemd_failed: 0,
  systemd_units: [{ unit: 'sshd.service', load: 'loaded', active: 'active', sub: 'running', description: 'OpenSSH server' }],
  journal_errors_1h: 2,
  journal_recent: ['sshd[1234]: Failed password for invalid user admin'],
  packagekit: {
    available: true,
    running: true,
    version: '1.2.6',
    summary: 'PackageKit daemon active',
  },
  summary: 'SELinux enforcing · tuned virtual-guest · PackageKit active · 0 failed units',
}

const mockConsoleHubSessions = [
  {
    session_id: '00000000-0000-4000-8000-000000000001',
    actor: 'admin',
    protocol: 'novnc',
    backend: 'libvirt',
    started_at: new Date(Date.now() - 3_600_000).toISOString(),
    ended_at: new Date(Date.now() - 1_800_000).toISOString(),
    recording_enabled: true,
    recording_path: '/var/lib/machina/console-recordings/00000000-0000-4000-8000-000000000001.webm',
    replay_available: true,
  },
  {
    session_id: '00000000-0000-4000-8000-000000000002',
    actor: 'admin',
    protocol: 'serial',
    backend: 'libvirt',
    started_at: new Date(Date.now() - 600_000).toISOString(),
    ended_at: null,
    recording_enabled: false,
  },
]

const sampleVm = {
  id: 'v1',
  name: 'vm-1',
  host_id: 'h1',
  observed_state: 'running',
  desired_state: 'running',
  lifecycle_phase: 'ready',
  managed: true,
  vcpus: 2,
  memory_mib: 2048,
  ha_enabled: false,
  project: null,
  tags: [],
  inventory_source: 'libvirt',
  guest_ip: '192.168.122.50',
  guest_tools_status: 'healthy',
}

const kubevirtVmFixture = {
  ...sampleVm,
  id: 'kv1',
  name: 'kv-vm-1',
  host_id: 'h1',
  inventory_source: 'kubevirt',
  k8s_namespace: 'default',
}

const spiceVmFixture = {
  ...sampleVm,
  id: 'sp1',
  name: 'spice-vm-1',
}

function k8sNodeFixture(name: string, plane: 'worker' | 'control_plane' | 'mixed', ready = true) {
  const roles =
    plane === 'control_plane' ? ['control-plane'] : plane === 'mixed' ? ['control-plane', 'worker'] : ['worker']
  return {
    name,
    roles,
    ready,
    kubelet_version: 'v1.29.0',
    os_image: 'Debian GNU/Linux 12 (bookworm)',
    kernel_version: '6.1.0-18-amd64',
    container_runtime: 'containerd://1.7.11',
    architecture: 'amd64',
    capacity: { cpu: '4', memory: '8Gi', pods: '110' },
    allocatable: { cpu: '3800m', memory: '7500Mi', pods: '110' },
    labels: { 'kubernetes.io/hostname': name },
    plane,
    cpu_capacity_millicores: 4000,
    cpu_allocatable_millicores: 3800,
    memory_capacity_bytes: 8e9,
    memory_allocatable_bytes: 7.5e9,
  }
}

function k8sPlaneRollup(nodeCount: number, readyCount = nodeCount) {
  return {
    node_count: nodeCount,
    ready_node_count: readyCount,
    cpu_capacity_millicores: 4000 * nodeCount,
    cpu_allocatable_millicores: 3800 * nodeCount,
    memory_capacity_bytes: 8e9 * nodeCount,
    memory_allocatable_bytes: 7.5e9 * nodeCount,
  }
}

const k8sEnvironmentFixture = {
  kubectl_on_path: true,
  kubectl_client_version: 'v1.29.0',
  kubectl_server_reachable: true,
  kubeconfig_hint: '~/.kube/config',
  kubeconfig_from_env: false,
  current_context: 'default',
  cluster_distribution: 'k3s',
  cluster_distribution_hints: ['k3s data dir present'],
  host: {
    k3s_config_present: true,
    k3s_data_dir_present: true,
    rke2_config_present: false,
    rke2_data_dir_present: false,
    k3s_systemd: 'active',
    k3s_agent_systemd: 'inactive',
    rke2_server_systemd: 'inactive',
    rke2_agent_systemd: 'inactive',
    k3s_binary_version: 'v1.29.0+k3s1',
    rke2_binary_version: null,
    helm_version: 'v3.14.0',
    crictl_version: 'v1.29.0',
  },
  snippets: {},
}

const placementRecommendation = {
  vm_id: 'v1',
  vm_name: 'vm-1',
  from_host_id: 'h1',
  from_host_name: 'host-1',
  to_host_id: 'h2',
  to_host_name: 'host-2',
  reason: 'CPU pressure on source host',
  score: 8.5,
}

export async function mockPlatformApi(page: Page, opts?: {
  tier?: 'normal' | 'power' | 'advanced'
  staleHost?: boolean
  emptyStorage?: boolean
  emptyIsos?: boolean
  templateNotReady?: boolean
  templateAutoFetch?: boolean
  emptyNetworks?: boolean
  stoppedVm?: boolean
  placementRecommendations?: boolean
  hostMaintenanceMode?: boolean
  emptyRunbooks?: boolean
}) {
  const tier = opts?.tier ?? 'normal'
  const hostMaintenanceMode = opts?.hostMaintenanceMode ?? true
  let hostDetail: Record<string, unknown> = { ...sampleHostDetail, maintenance_mode: hostMaintenanceMode }
  const listHost = { ...sampleHost, maintenance_mode: hostMaintenanceMode }
  const maintenanceMission = hostMaintenanceMode
    ? fleetMaintenanceMission
    : {
        ...fleetMaintenanceMission,
        summary: '1 host(s) with updates · 0 in maintenance · 0 pending schedule(s)',
        hosts_in_maintenance: 0,
        hosts: fleetMaintenanceMission.hosts.map((h) => ({
          ...h,
          maintenance_mode: false,
          steps: h.steps.map((s) =>
            s.id === 'enter_maintenance' ? { ...s, status: 'ready' as const } : s,
          ),
        })),
      }
  const vmFixture = opts?.stoppedVm
    ? { ...sampleVm, observed_state: 'stopped', desired_state: 'stopped', lifecycle_phase: 'idle' }
    : sampleVm
  const portForwardRules: Array<{
    id: string
    protocol: string
    host_port: number
    vm_ip: string
    vm_port: number
    description: string
  }> = [
    {
      id: 'pf-1',
      protocol: 'tcp',
      host_port: 9080,
      vm_ip: '192.168.122.10',
      vm_port: 80,
      description: 'vm-1',
    },
  ]
  let promptTitle = 'RCA template'
  let storagePools: Array<{ id: string; name: string; path: string; capacity_gib: number; used_gib: number }> =
    opts?.emptyStorage ? [] : [{ id: 'p1', name: 'default', path: '/var/lib/libvirt/images', capacity_gib: 500, used_gib: 12 }]
  // Classic daemon storage.ts's StoragePoolInfo shape (gb, not gib; allocation/available/state/
  // autostart) — a distinct type from the platform/controller StoragePool above (`storagePools`),
  // which api/storage.ts's listPools() (GET /api/v1/storage/pools) never reads. Reusing that
  // wrongly-shaped variable here used to leave every numeric field undefined, and Storage.tsx's
  // unguarded .toFixed() calls on them crashed the whole app through AppErrorBoundary.
  const classicStoragePools = opts?.emptyStorage ? [] : [
    { name: 'default', uuid: 'a1b2c3d4-0000-0000-0000-000000000001', state: 'running', capacity_gb: 500, allocation_gb: 80, available_gb: 420, autostart: true },
    { name: 'data', uuid: 'a1b2c3d4-0000-0000-0000-000000000002', state: 'running', capacity_gb: 1000, allocation_gb: 100, available_gb: 900, autostart: false },
  ]
  let templateReadinessPolls = 0
  await page.addInitScript((t) => {
    localStorage.setItem('zyvor-platform-welcome-done', '1')
    localStorage.setItem('machina-platform-desktop-tier', t)
    localStorage.removeItem('machina_platform_controller')
    try {
      sessionStorage.removeItem('machina-fleet-insights-expanded')
    } catch {
      /* ignore */
    }
  }, tier)

  await page.route('**/api/v1/**', async (route) => {
    const url = route.request().url()
    if (url.includes('/auth/session')) {
      return route.fulfill({
        json: { authenticated: true, username: 'admin', role: 'admin', auth_source: 'pam' },
      })
    }
    if (url.includes('/auth/providers')) {
      return route.fulfill({
        json: { pam: { enabled: true }, ldap: { enabled: false }, oidc: { enabled: false } },
      })
    }
    if (url.includes('/auth/login') && route.request().method() === 'POST') {
      return route.fulfill({
        json: { token: 'mock-login-jwt-token', username: 'admin', role: 'admin' },
      })
    }
    if (url.includes('/system/auth/ldap-settings')) {
      return route.fulfill({
        json: {
          enabled: false,
          url: '',
          bind_dn: '',
          bind_password: '',
          bind_password_set: false,
          user_search_base: '',
          user_search_filter: '',
          admin_group_substrings: [],
          operator_group_substrings: [],
          readonly_group_substrings: [],
          config_path: '',
          preset: null,
        },
      })
    }
    if (url.includes('/ws-token') && route.request().method() === 'POST') {
      return route.fulfill({ json: { token: 'mock-ws-token' } })
    }
    if (url.includes('/system/platform-info')) {
      return route.fulfill({ json: platformInfo })
    }
    if (url.includes('/api/v1/events') && !url.includes('/events/stream')) {
      return route.fulfill({ json: platformEvents })
    }
    if (url.match(/\/api\/v1\/health(\?|$)/)) {
      return route.fulfill({
        json: { status: 'ok', leader: true, controller_id: 'ctrl-test-1' },
      })
    }
    if (url.match(/\/hosts\/[^/]+\/linux\/observability/)) {
      return route.fulfill({
        json: {
          pressure: {
            cpu: { some: 0.02, full: 0.01, total: 0.03 },
            memory: { some: 0.01, full: 0, total: 0.02 },
            io: { some: 0.03, full: 0, total: 0.04 },
          },
          disk_io: [{ device: 'sda', read_bytes: 1e9, write_bytes: 2e8 }],
          thermal: [],
          smart: [],
          bpf: { programs_loaded: 2, events_per_sec: 12 },
          cgroup: { memory_current: 1e9, cpu_usage_usec: 5e8 },
          vm_cgroups: [{ vm_name: 'vm-1', memory_bytes: 2e9 }],
        },
      })
    }
    if (url.match(/\/hosts\/[^/]+\/linux\/(network-diag|audit|filesystems|processes|updates)/)) {
      if (url.includes('/network-diag')) {
        return route.fulfill({
          json: {
            networkd_active: true,
            resolved_active: true,
            interfaces: [{ name: 'eth0', state: 'routable', addresses: ['10.0.0.1'] }],
            networkctl_status: '● systemd-networkd.service - Network Configuration\n   Active: active',
            resolvectl_status: 'Global\n       DNS Servers: 10.0.0.53',
          },
        })
      }
      if (url.includes('/audit')) {
        return route.fulfill({
          json: {
            available: true,
            auditd_active: true,
            avc_count: 1,
            events: [
              { type: 'avc', message: 'denied { read } for pid=1234 comm="curl"', summary: 'denied { read } for pid=1234' },
            ],
            summary: '1 recent AVC event',
          },
        })
      }
      if (url.includes('/filesystems')) {
        return route.fulfill({
          json: {
            filesystems: [{
              mount_point: '/',
              fstype: 'ext4',
              source: '/dev/sda1',
              size_bytes: 40e9,
              used_bytes: 12e9,
              avail_bytes: 28e9,
              use_percent: 30,
            }],
          },
        })
      }
      if (url.includes('/processes')) {
        return route.fulfill({
          json: {
            processes: [{ pid: 1, user: 'root', cpu_percent: 0.1, rss_kb: 10240, command: 'systemd' }],
          },
        })
      }
      return route.fulfill({
        json: {
          summary: '3 packages pending',
          backend: 'apt',
          pending_count: 3,
          reboot_required: true,
          packages: [
            { name: 'libc6', current: '2.35', available: '2.36', security: true },
            { name: 'openssl', current: '3.0.2', available: '3.0.3', security: true },
          ],
        },
      })
    }
    if (url.match(/\/hosts\/[^/]+\/linux\/package-upgrade/) && route.request().method() === 'POST') {
      const body = route.request().postDataJSON() as { dry_run?: boolean } | null
      if (body?.dry_run) {
        return route.fulfill({
          json: {
            dry_run: true,
            summary: '3 packages would be upgraded (libc6, openssl, curl)',
            result: { ok: true, stdout: 'Inst libc6 [2.36]\nInst openssl [3.0.3]' },
          },
        })
      }
      return route.fulfill({
        json: { task_id: 'task-linux-upgrade-1', summary: 'Linux package upgrade queued' },
      })
    }
    if (url.match(/\/hosts\/[^/]+\/linux\/reboot/) && route.request().method() === 'POST') {
      return route.fulfill({
        json: { task_id: 'task-linux-reboot-1', summary: 'Host reboot queued' },
      })
    }
    if (url.match(/\/hosts\/[^/]+\/lldp/)) {
      return route.fulfill({
        json: { source: 'lldpctl', neighbors: [], raw_text: '', summary: 'No LLDP neighbors' },
      })
    }
    if (url.match(/\/hosts\/[^/]+\/cockpit\/actions/) && route.request().method() === 'POST') {
      return route.fulfill({ json: { status: 'ok', message: 'Applied (mock)' } })
    }
    if (url.match(/\/hosts\/[^/]+\/cockpit(\?|$)/)) {
      const section = new URL(url, 'http://mock.local').searchParams.get('section') || 'all'
      const hostId = url.match(/\/hosts\/([^/]+)\/cockpit/)?.[1] ?? 'h1'
      const body: Record<string, unknown> = { host_id: hostId }
      if (section === 'all' || section === 'storage') body.storage = mockHostCockpitStorage
      if (section === 'all' || section === 'network') body.network = mockHostCockpitNetwork
      if (section === 'all' || section === 'system') body.system = mockHostCockpitSystem
      return route.fulfill({ json: body })
    }
    if (url.includes('/zeus-firewall/status')) {
      return route.fulfill({
        json: {
          zeus_firewall: { enabled: true, agent_count: 1, summary: 'Zeus Firewall daemon active' },
          native_bpf: { summary: 'machina-bpfd healthy on 2/2 host(s)' },
        },
      })
    }
    if (url.match(/\/zeus-firewall\/targets\/[^/]+\/score/)) {
      return route.fulfill({
        json: {
          score: 78,
          breakdown: [{ category: 'exposure', status: 'warn', points: -12, detail: '3 high-risk ports open' }],
          recommendations: [{ label: 'Close port 4444', points: 8, action: 'deny' }],
        },
      })
    }
    if (url.includes('/zeus-firewall/approvals') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          id: 'apr-1',
          target_id: 'h1',
          status: 'pending',
          profile: 'ProductionServer',
          created_at: new Date().toISOString(),
        },
      })
    }
    if (url.includes('/zeus-firewall/profiles')) {
      // Same shape-mismatch risk as /approvals above — listFirewallProfiles() expects an array.
      return route.fulfill({ json: [{ name: 'ProductionServer', display_name: 'Production Server', description: '' }] })
    }
    if (url.includes('/zeus-firewall/approvals')) {
      // Otherwise falls through to the generic '/zeus-firewall/' catch-all below, which returns an
      // object ({summary,targets,profiles}) — listFirewallApprovals() expects an array, and
      // PlatformFirewallCompliance.tsx calls .map() on it directly, so that shape mismatch crashed
      // the whole page.
      return route.fulfill({ json: [] })
    }
    if (url.includes('/zeus-firewall/overview')) {
      return route.fulfill({
        json: {
          summary: '1 target monitored',
          critical_count: 1,
          warning_count: 0,
          profiles: ['ProductionServer'],
          targets: [{
            id: 'h1',
            name: 'host-1',
            hostname: 'host-1',
            kind: 'host',
            risk: 'critical',
            score: 72,
            open_ports: 3,
            enabled: true,
            backend: 'nftables',
            agent_reachable: true,
            blocked_today: 0,
            profile: 'ProductionServer',
          }],
        },
      })
    }
    if (url.includes('/zeus-firewall/multisite/overview')) {
      return route.fulfill({
        json: {
          summary: '2 sites federated',
          sites: [
            { id: 's1', name: 'primary-local', role: 'primary', gitops_namespace: 'zeus-primary', target_count: 1 },
            { id: 's2', name: 'dr-replica', role: 'dr', gitops_namespace: 'zeus-dr', target_count: 1 },
          ],
          compliance_rollup: { sites: [{ site: 'primary-local', grade: 'A' }, { site: 'dr-replica', grade: 'B' }] },
          policy_conflicts: [],
        },
      })
    }
    if (url.includes('/zeus-firewall/operator/plan')) {
      return route.fulfill({
        json: {
          summary: '1 host auto-eligible',
          auto_eligible: 1,
          approval_required: 0,
          previews: [{
            host_id: 'h1',
            hostname: 'host-1',
            current_score: 72,
            target_profile: 'ProductionServer',
            predicted_score: 90,
            risk: 'medium',
            monthly_exposure_usd: 12,
            requires_approval: false,
            summary: 'Apply ProductionServer profile',
          }],
        },
      })
    }
    if (url.includes('/zeus-firewall/operator/execute') && !url.includes('execute-batch') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          dry_run: true,
          host_id: 'h1',
          applied: false,
          operations: 2,
          message: 'Dry-run: would apply ProductionServer profile on host-1',
        },
      })
    }
    if (url.includes('/zeus-firewall/operator/thresholds')) {
      return route.fulfill({
        json: {
          summary: 'Operator thresholds',
          min_score_auto: 70,
          max_open_ports: 5,
        },
      })
    }
    if (url.includes('/zeus-firewall/baremetal/overview')) {
      return route.fulfill({
        json: { summary: '0 bare-metal targets', targets: [], scans_pending: 0 },
      })
    }
    if (url.match(/\/platform\/controller\/api\/v1\/vms(\?|$)/)) {
      return route.fulfill({
        json: [
          // v1 has no guest_ip so Machine Finder's batch guest-ip lookup (below, keyed off
          // vmFixture.id === 'v1') actually fires and the fallback hint is genuinely exercised,
          // instead of the primary guest_ip masking it every time.
          { id: 'v1', name: 'web-01', observed_state: 'running', host_id: 'h1', guest_ip: null, inventory_source: 'libvirt' },
          { id: 'v2', name: 'db-01', observed_state: 'stopped', host_id: 'h1', guest_ip: '192.168.122.11', inventory_source: 'libvirt' },
        ],
      })
    }
    if (url.match(/\/platform\/controller\/api\/v1\/networks(\?|$)/) && route.request().method() === 'GET') {
      // Shares the same PlatformNetwork[] fixture as the '/networks' handler further below (which
      // this regex-matched, platform/controller-proxied URL never reaches — it's caught here first).
      // A previous hardcoded `[{ id: 'n1', name: 'private' }]` here lacked `backend`, which crashed
      // PlatformNetworks.tsx's table row (`t.backend.replace(...)`) on every visit to /platform/networks.
      // Scoped to GET so the POST-create handler further below still gets a turn.
      return route.fulfill({ json: opts?.emptyNetworks ? [] : platformNetworks })
    }
    if (url.includes('/k8s/overview')) {
      return route.fulfill({
        json: {
          version: 'v1.29.0',
          nodes: 3,
          ready_nodes: 3,
          namespaces: 8,
          pods: 42,
          deployments: 12,
          services: 18,
          distribution: 'k3s',
        },
      })
    }
    if (url.includes('/k8s/nodes')) {
      return route.fulfill({
        json: [k8sNodeFixture('node-1', 'worker'), k8sNodeFixture('node-2', 'control_plane')],
      })
    }
    if (url.includes('/k8s/cluster-inventory/history')) {
      return route.fulfill({ json: { path: '/var/lib/machina/k8s-inventory.jsonl', entries: [] } })
    }
    if (url.includes('/k8s/cluster-inventory')) {
      return route.fulfill({
        json: {
          collected_at_rfc3339: '2026-06-01T12:00:00Z',
          disclaimer: 'E2E cluster inventory snapshot',
          nodes: [k8sNodeFixture('node-1', 'worker')],
          totals_all_nodes: k8sPlaneRollup(1),
          combined_control_plane_and_mixed: k8sPlaneRollup(0),
          combined_worker_dataplane_and_mixed: k8sPlaneRollup(1),
          by_plane: { worker: k8sPlaneRollup(1) },
          nodes_with_kubelet_minor_skew: 0,
          topology_nodes_by_zone: { 'zone-a': 1 },
          topology_nodes_by_region: {},
          apiserver_major_minor: '1.29',
          apiserver_git_version: 'v1.29.0+k3s1',
          cluster_livez_ok: true,
          cluster_readyz_ok: true,
        },
      })
    }
    if (url.includes('/k8s/environment')) {
      return route.fulfill({ json: k8sEnvironmentFixture })
    }
    if (url.includes('/k8s/metrics')) {
      return route.fulfill({
        json: { metrics_available: false, nodes_top: [], pods_top: [] },
      })
    }
    if (url.match(/\/k8s\/kubevirt\/virtualmachines\/[^/]+\/[^/]+\/lifecycle/) && route.request().method() === 'POST') {
      const body = route.request().postDataJSON() as { action?: string } | null
      return route.fulfill({
        json: { ok: true, action: body?.action ?? 'start', namespace: 'default', name: 'kv-vm-1' },
      })
    }
    if (url.match(/\/k8s\/kubevirt\/virtualmachines\/[^/]+\/[^/]+$/) && route.request().method() === 'DELETE') {
      return route.fulfill({ json: { deleted: true, namespace: 'default', name: 'kv-vm-1' } })
    }
    if (url.includes('/k8s/kubevirt/virtualmachines') && route.request().method() === 'GET') {
      return route.fulfill({
        json: {
          items: [
            {
              metadata: { name: 'kv-vm-1', namespace: 'default' },
              spec: { running: true },
              status: { printableStatus: 'Running', ready: true },
            },
          ],
        },
      })
    }
    if (url.includes('/k8s/kubevirt/vm-summary')) {
      return route.fulfill({
        json: [
          {
            name: 'kv-vm-1',
            namespace: 'default',
            spec_running: true,
            vm_printable_status: 'Running',
            vm_ready: true,
            guest_ip: '10.244.0.18',
            pod_ip: '10.244.0.18',
            vmi_phase: 'Running',
            node_name: 'worker-1',
            virtctl_console: 'virtctl console kv-vm-1 -n default',
            virtctl_vnc: 'virtctl vnc kv-vm-1 -n default',
            virtctl_vnc_socks: 'virtctl vnc kv-vm-1 -n default --proxy-only',
            vnc_subresource_path: '/apis/subresources.kubevirt.io/v1/namespaces/default/virtualmachineinstances/kv-vm-1/vnc',
          },
        ],
      })
    }
    if (url.match(/\/k8s\/(namespaces|deployments|pods|services|statefulsets|daemonsets|jobs)(\?|$)/)) {
      return route.fulfill({ json: { items: [] } })
    }
    if (url.includes('/k8s/contexts')) {
      return route.fulfill({ json: { contexts: ['default'] } })
    }
    if (url.includes('/cloud-init') && route.request().method() === 'POST') {
      return route.fulfill({
        json: { status: 'ok', path: '/var/lib/libvirt/images/seed-e2e.iso' },
      })
    }
    if (url.includes('/platform/controller') && /\/api\/v1\/templates\/?(\?.*)?$/.test(url)) {
      return route.fulfill({ json: [sampleTemplate, fleetOnlyTemplate] })
    }
    if (!url.includes('/platform/controller') && /\/api\/v1\/templates\/?(\?.*)?$/.test(url)) {
      return route.fulfill({ json: [libvirtTemplateFixture] })
    }
    if (url.includes('/templates/saved')) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/fleet/prometheus-targets')) {
      return route.fulfill({
        json: {
          enabled: true,
          metrics_path: '/api/v1/fleet/prometheus',
          note: 'E2E fleet Prometheus scrape configs',
          scrape_configs: [{ job_name: 'machina-fleet', targets: ['127.0.0.1:8788'] }],
        },
      })
    }
    if (url.includes('/fleet/status')) {
      return route.fulfill({
        json: { enabled: true, peers: [], primary_peer: '', standby_peer: '' },
      })
    }
    if (url.includes('/fleet/metrics')) {
      return route.fulfill({
        json: {
          enabled: true,
          local: { host_cpu_percent: 20, host_memory_percent: 40, load_1: 0.5, vm_count: 1, vms_running: 1 },
          peers: [],
        },
      })
    }
    if (url.includes('/fleet/vms')) {
      return route.fulfill({ json: { enabled: true, vms: [] } })
    }
    if (url.includes('/fleet/alerts')) {
      return route.fulfill({ json: { peers: [], total_unacknowledged: 0 } })
    }
    if (url.includes('/libvirt/summary')) {
      return route.fulfill({
        json: {
          configured_uri: 'qemu:///system',
          libvirt_connected: true,
          libvirt_system_socket_present: true,
          libvirt_session_socket_present: false,
        },
      })
    }
    if (url.includes('/guest-images/os-list')) {
      return route.fulfill({
        json: { oses: [{ short_id: 'alma9', name: 'AlmaLinux', version: '9' }] },
      })
    }
    if (url.includes('/system/create-vm-defaults')) {
      return route.fulfill({ json: {} })
    }
    if (url.includes('/guest-images/rhel-url') && route.request().method() === 'POST') {
      return route.fulfill({
        json: { raw: { href: 'https://access.redhat.com/downloads/content/e2e-rhel-9' } },
      })
    }
    if (url.includes('/kubevirt/qcow2-bundle') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          libvirt_vm: 'qcow2-import',
          libvirt_root_disk: '/var/lib/libvirt/images/e2e.qcow2',
          namespace: 'default',
          virtual_machine_name: 'kv-e2e',
          datavolume_name: 'dv-e2e',
          upload_size_gi: 20,
          cluster_exec_enabled: true,
          yaml: 'apiVersion: kubevirt.io/v1\nkind: VirtualMachine\n',
          virtctl_image_upload_example: 'virtctl image-upload dv-e2e',
        },
      })
    }
    if (url.includes('/kubevirt/qcow2-bundle')) {
      return route.fulfill({
        json: {
          libvirt_vm: 'qcow2-import',
          libvirt_root_disk: '/var/lib/libvirt/images/e2e.qcow2',
          namespace: 'default',
          virtual_machine_name: 'kv-e2e',
          datavolume_name: 'dv-e2e',
          upload_size_gi: 20,
          cluster_exec_enabled: true,
          yaml: 'apiVersion: kubevirt.io/v1\nkind: VirtualMachine\n',
          virtctl_image_upload_example: 'virtctl image-upload dv-e2e',
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/hostname/)) {
      return route.fulfill({ json: { hostname: 'vm-1-guest.local' } })
    }
    if (url.match(/\/backups\/[^/]+\/status/)) {
      return route.fulfill({
        json: {
          backup_id: 'b1',
          status: 'running',
          message: 'Copying qcow2 images (42%)',
          progress: '42%',
          updated: new Date().toISOString(),
        },
      })
    }
    if (url.includes('/backups/schedule')) {
      return route.fulfill({
        json: { installed: true, enabled: false, active: false, next_run: '', last_run: '' },
      })
    }
    if (url.match(/\/api\/v1\/backups(\?|$)/)) {
      return route.fulfill({
        json: [{
          id: 'b1',
          timestamp: new Date().toISOString(),
          vm_filter: 'all',
          vm_count: 2,
          net_count: 1,
          with_disks: true,
          nfs_target: 'local',
          size: '12 GiB',
          status: 'running',
          status_message: '',
          progress: '10',
          has_checksums: false,
        }],
      })
    }
    if (url.includes('/browse/isos')) {
      return route.fulfill({
        json: opts?.emptyIsos
          ? { files: [], scan_directories: ['/var/lib/libvirt/images'] }
          : {
              files: [{ path: '/var/lib/libvirt/images/debian-12.iso', name: 'debian-12.iso', size_bytes: 8e8, format: 'iso' }],
              scan_directories: ['/var/lib/libvirt/images'],
            },
      })
    }
    if (url.includes('/browse/dir')) {
      const reqUrl = new URL(route.request().url())
      const dirPath = reqUrl.searchParams.get('path') ?? ''
      if (!dirPath) {
        return route.fulfill({
          json: {
            path: '',
            parent: null,
            roots: ['/var/lib/libvirt/images'],
            entries: [],
          },
        })
      }
      if (dirPath === '/var/lib/libvirt/images') {
        return route.fulfill({
          json: {
            path: '/var/lib/libvirt/images',
            parent: '/var/lib/libvirt',
            roots: ['/var/lib/libvirt/images'],
            entries: [
              {
                name: 'debian-12.iso',
                path: '/var/lib/libvirt/images/debian-12.iso',
                is_directory: false,
                size_bytes: 800000000,
              },
              {
                name: 'data.qcow2',
                path: '/var/lib/libvirt/images/data.qcow2',
                is_directory: false,
                size_bytes: 5000000000,
              },
            ],
          },
        })
      }
      return route.fulfill({
        json: {
          path: dirPath,
          parent: '/var/lib/libvirt/images',
          roots: ['/var/lib/libvirt/images'],
          entries: [],
        },
      })
    }
    if (url.includes('/browse/disks')) {
      return route.fulfill({
        json: {
          files: [{ path: '/var/lib/libvirt/images/ubuntu.qcow2', name: 'ubuntu.qcow2', size_bytes: 5e9, format: 'qcow2' }],
          scan_directories: ['/var/lib/libvirt/images'],
        },
      })
    }
    if (url.includes('/browse/virt-image-output-roots')) {
      return route.fulfill({
        json: { allowed_prefixes: ['/var/lib/libvirt/images'], effective_tmpdir: '/var/tmp' },
      })
    }
    if (url.match(/\/browse\/virt-builder\/probe\//)) {
      return route.fulfill({
        json: { virt_builder_allowed: true, name_valid: true, in_cached_catalog: true, hint: 'Template ready for build' },
      })
    }
    if (url.match(/\/browse\/virt-builder\/notes\//)) {
      return route.fulfill({
        json: { template: 'debian-12', notes: 'Debian 12 stable — minimal install, cloud-init friendly.' },
      })
    }
    if (url.includes('/browse/virt-image-build') && route.request().method() === 'POST') {
      return route.fulfill({
        json: { status: 'ok', path: '/var/lib/libvirt/images/e2e-built.qcow2' },
      })
    }
    if (url.includes('/browse/mkosi-workspaces')) {
      return route.fulfill({
        json: [{
          path: '/var/lib/machina/mkosi-defs/fedora',
          name: 'fedora',
          images: ['base', 'tools'],
        }],
      })
    }
    if (url.includes('/browse/virt-builder')) {
      return route.fulfill({
        json: {
          virt_builder_allowed: true,
          virt_builder_installed: true,
          templates: ['debian-12', 'ubuntu-24.04'],
          items: [{ name: 'debian-12' }, { name: 'ubuntu-24.04' }],
        },
      })
    }
    if (url.includes('/fleet/console')) {
      return route.fulfill({
        json: {
          summary: '2 events in last 24h',
          total_24h: 2,
          audit_24h: 1,
          events_24h: 1,
          task_failures_24h: 0,
          entries: [
            {
              id: 'e1',
              created_at: new Date().toISOString(),
              severity: 'info',
              source: 'audit',
              action: 'login',
              message: 'user login',
              actor: 'admin',
            },
          ],
        },
      })
    }
    if (url.includes('/events/stream') || url.includes('/ws/')) {
      return route.abort()
    }
    if (url.includes('/fleet/finder')) {
      return route.fulfill({ json: fleetFinder })
    }
    if (url.includes('/network-canvas')) {
      return route.fulfill({
        json: {
          topology: {
            nodes: [{ id: 'h1', name: 'host-1', kind: 'host', state: 'online' }],
            edges: [],
          },
          flows: { flows: [] },
          flow_stats: { total_flows: 50, denied_flows: 2, tx_bytes: 4096, rx_bytes: 8192 },
          anomalies: { anomalies: [{ summary: 'Unusual east-west traffic', severity: 'medium', host_id: 'host-1' }] },
          native_bpf: { enabled: true, reachable: true, hosts_total: 2, hosts_reachable: 2, enforcing_hosts: 0, version: '0.1.0', summary: 'machina-bpfd healthy on 2/2 host(s)' },
          network_pulse: {
            enabled: true,
            overview: { total_flows: 12, denied_flows: 1 },
            service_map: {
              nodes: [
                { name: 'web', namespace: 'default', status: 'ok', connections_in: 2, connections_out: 5, blocked_flows: 0, risk: 'low' },
                { name: 'db', namespace: 'default', status: 'warning', connections_in: 4, connections_out: 1, blocked_flows: 1, risk: 'medium' },
              ],
              edges: [
                { id: 'e1', source: 'default/web', target: 'default/db', health: 'ok', dropped_count: 0 },
              ],
              meta: { stats: { services: 2, connections: 1, blocked: 1, warnings: 1 } },
              overlays: { top_talker_nodes: ['web'], attack_path_workloads: ['db'] },
            },
            workloads: {
              workloads: [
                { namespace: 'default', name: 'web', connections_out: 5 },
                { namespace: 'default', name: 'db', connections_out: 1 },
              ],
            },
            timeline: {
              events: [
                { summary: 'New connection to db:5432', severity: 'info', timestamp: new Date().toISOString(), kind: 'network_connect' },
                { summary: 'Correlation: lateral movement pattern', severity: 'high', timestamp: new Date().toISOString(), kind: 'correlation' },
              ],
            },
            threats: {
              threats: [{ title: 'Suspicious shell', severity: 'high', summary: 'Reverse shell pattern on host-1', host_id: 'host-1' }],
            },
            top_talkers: { talkers: [{ name: 'default/web', flows: 12 }] },
            k8s_nodes: { nodes: [{ name: 'node-a', status: 'Ready', pods_count: 8 }] },
          },
        },
      })
    }
    if (url.includes('/fleet/maintenance-mission')) {
      return route.fulfill({ json: maintenanceMission })
    }
    if (url.match(/\/fleet\/mission(\?|$|\/)/)) {
      return route.fulfill({ json: fleetMission })
    }
    if (url.match(/\/fleet\/gpu(\?|$|\/)/)) {
      return route.fulfill({ json: fleetGpu })
    }
    if (url.match(/\/fleet\/dna(\?|$|\/)/)) {
      return route.fulfill({ json: fleetDna })
    }
    if (url.includes('/ai/settings')) {
      return route.fulfill({
        json: {
          enabled: true,
          mode: 'advisor',
          provider: 'local',
          model: 'default',
          api_key_configured: false,
          autopilot_interval_secs: 3600,
        },
      })
    }
    if (url.includes('/ai/jarvis/landing')) {
      return route.fulfill({ json: jarvisLanding })
    }
    if (url.includes('/ai/incidents/analyze')) {
      return route.fulfill({
        json: {
          window_hours: 4,
          root_cause: 'Network configuration change likely caused connectivity loss.',
          confidence: 0.76,
          contributing_factors: ['Firewall rule update'],
          suggested_actions: ['Review recent network policy', 'Run graph path analysis'],
          evidence: ['audit: firewall.update'],
          timeline: [
            { at: new Date().toISOString(), source: 'audit', kind: 'firewall', message: 'Rule updated', severity: 'high' },
          ],
        },
      })
    }
    if (url.includes('/ai/timeline/replay')) {
      return route.fulfill({
        json: {
          entries: [{ at: new Date().toISOString(), source: 'audit', kind: 'vm', message: 'VM created', severity: 'info' }],
          graph_changes: ['+1 vm ubuntu-desktop'],
        },
      })
    }
    if (url.match(/\/ai\/graph(\?|$)/) && route.request().method() === 'GET') {
      return route.fulfill({
        json: {
          nodes: [
            { kind: 'host', id: 'h1', name: 'host-1', state: 'online', health_score: 88 },
            { kind: 'vm', id: 'v1', name: 'ubuntu-desktop', state: 'running', health_score: 90 },
            { kind: 'vm', id: 'v2', name: 'db-01', state: 'running', health_score: 85 },
          ],
          edges: [
            { from: 'h1', to: 'v1', label: 'runs' },
            { from: 'v1', to: 'network-br-default', label: 'connected_to' },
          ],
          node_count: 3,
          edge_count: 2,
        },
      })
    }
    if (url.match(/\/ai\/graph\/at\//) && route.request().method() === 'GET') {
      return route.fulfill({
        json: {
          timestamp: new Date().toISOString(),
          nodes: [
            { kind: 'host', id: 'h1', name: 'host-1', state: 'online', health_score: 88 },
            { kind: 'vm', id: 'v1', name: 'ubuntu-desktop', state: 'running', health_score: 90 },
          ],
          edges: [{ from: 'h1', to: 'v1', label: 'runs' }],
          diff_summary: '-1 vm db-01 vs now',
          current_node_count: 3,
          node_delta: -1,
          added_nodes: [],
          removed_nodes: ['vm:db-01'],
        },
      })
    }
    if (url.match(/\/ai\/graph\/object\//) && route.request().method() === 'GET') {
      return route.fulfill({
        json: {
          kind: 'vm',
          id: 'v1',
          name: 'ubuntu-desktop',
          purpose: 'Primary desktop VM on default bridge.',
          risks: ['No recent backup snapshot'],
          health_score: 90,
        },
      })
    }
    if (url.includes('/ai/graph/path') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          can_reach: false,
          explanation: 'VMs on different segments — verify firewall for port 5432.',
          hops: ['ubuntu-desktop', 'host h1', 'cluster network', 'db-01'],
          blockers: [{ kind: 'segment_mismatch', message: 'Different bridges', remediation: 'Check VLAN routing' }],
          confidence: 0.88,
          evidence: [{ source: 'graph', detail: 'No shared bridge' }],
        },
      })
    }
    if (url.includes('/ai/predictions')) {
      return route.fulfill({
        json: {
          summary: '1 prediction',
          predictions: [{ resource: 'storage', resource_kind: 'storage', kind: 'exhaustion', severity: 'high', message: 'Pool 87% full', hours_until_critical: 72, confidence: 0.72, evidence: 'capacity' }],
        },
      })
    }
    if (url.includes('/ai/troubleshoot')) {
      return route.fulfill({
        json: {
          vm_id: 'v1', vm_name: 'ubuntu-desktop', symptom: 'slow', severity: 'medium',
          checks: [
            { domain: 'cpu', status: 'ok', detail: 'CPU 45%' },
            { domain: 'memory', status: 'warn', detail: '78% used' },
            { domain: 'disk', status: 'ok', detail: '1 disk' },
            { domain: 'host_pressure', status: 'ok', detail: 'Host OK' },
            { domain: 'network', status: 'ok', detail: 'No symptom' },
          ],
          findings: [{ severity: 'medium', message: 'Memory pressure', domain: 'memory' }],
          recommended_actions: ['Increase RAM'],
        },
      })
    }
    if (url.includes('/ai/rightsizing/report')) {
      return route.fulfill({
        json: {
          recommendations: [{ vm_id: 'v1', vm_name: 'idle-vm', action: 'power_off', detail: 'Stopped 30d', savings_usd: 15, risk: 'medium', current_memory_mib: 0, suggested_memory_mib: 0 }],
          idle_vm_count: 1, oversized_vm_count: 2, estimated_monthly_savings_usd: 45,
        },
      })
    }
    if (url.includes('/ai/memory/incidents')) {
      return route.fulfill({
        json: {
          incidents: [{ at: new Date().toISOString(), kind: 'rca', summary: 'Network change', actor: 'admin', lesson: 'Review firewall rules' }],
          runbook_hints: ['Check bridge state after NIC events'],
        },
      })
    }
    if (url.includes('/ai/memory/changes-before')) {
      return route.fulfill({
        json: {
          summary: '1 change in the 4h before incident.',
          changes: [{ at: new Date().toISOString(), kind: 'firewall.update', summary: 'Rule updated', actor: 'admin' }],
        },
      })
    }
    if (url.includes('/ai/incidents/active')) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/ai/actions/hub')) {
      return route.fulfill({
        json: {
          zyra_actions: [
            {
              id: 'act-1',
              source: 'nl_ops',
              action_type: 'vm.restart',
              label: 'Restart idle-vm',
              review: 'VM stopped 30 days — safe restart candidate',
              risk: 'low',
              status: 'pending',
            },
          ],
          total_pending: 1,
          firewall_pending: 0,
        },
      })
    }
    if (url.includes('/ai/agents')) {
      return route.fulfill({
        json: [{ id: 'fleet', name: 'Fleet Agent', description: 'Autonomous fleet ops', task_class: 'fleet' }],
      })
    }
    if (url.includes('/ai/zyra/plan') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          goal: 'Rebalance idle VMs and clear failed tasks',
          agent_id: 'fleet',
          steps: [
            { title: 'Identify idle VMs', detail: 'Scan fleet for stopped guests' },
            { title: 'Queue rebalance moves', detail: 'Relieve cold hosts' },
          ],
        },
      })
    }
    if (url.includes('/ai/zyra/execute') && route.request().method() === 'POST') {
      return route.fulfill({ json: { message: 'Queued 2 steps for approval' } })
    }
    if (url.includes('/ai/policy/export')) {
      return route.fulfill({ json: { yaml: 'rules:\n- name: default', rule_count: 1, quota_count: 1 } })
    }
    if (url.includes('/ai/terminal/suggest') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          vm_name: vmFixture.name,
          observed_state: vmFixture.observed_state,
          suggestions: [
            { label: 'Check disk', command: 'df -h', description: 'Disk usage on guest', scope: 'guest' },
          ],
          notes: 'Mock terminal suggestions',
        },
      })
    }
    if (url.includes('/ai/routing/rules') && route.request().method() === 'PATCH') {
      return route.fulfill({
        json: { task_class: 'infrastructure', provider_id: 'prov-1', model_id: 'mod-1', enabled: true },
      })
    }
    if (url.includes('/ai/routing/rules')) {
      return route.fulfill({
        json: [
          { task_class: 'infrastructure', provider_id: 'prov-1', model_id: 'mod-1', enabled: true },
          { task_class: 'fast_local', provider_id: null, model_id: null, enabled: true },
        ],
      })
    }
    if (url.includes('/ai/enterprise/zeus') && route.request().method() === 'PATCH') {
      return route.fulfill({
        json: {
          zeus_admin_role: true,
          zeus_execute_role: true,
          zeus_read_role: true,
          air_gap_llm: true,
          audit_events_24h: 42,
          scim_enabled: false,
          sso_configured: false,
        },
      })
    }
    if (url.includes('/ai/enterprise/zeus')) {
      return route.fulfill({
        json: {
          zeus_admin_role: true,
          zeus_execute_role: true,
          zeus_read_role: true,
          air_gap_llm: false,
          audit_events_24h: 42,
          scim_enabled: false,
          sso_configured: false,
        },
      })
    }
    if (url.includes('/ai/memory/settings') && route.request().method() === 'PATCH') {
      return route.fulfill({
        json: { enabled: true, team_scope: false, project_scope: true, retention_days: 90 },
      })
    }
    if (url.includes('/ai/memory/settings')) {
      return route.fulfill({
        json: { enabled: true, team_scope: false, project_scope: true, retention_days: 90 },
      })
    }
    if (url.match(/\/ai\/memory(\?|$)/) && route.request().method() === 'DELETE') {
      return route.fulfill({ json: { deleted: 5 } })
    }
    if (url.match(/\/ai\/prompts\/[^/]+$/) && route.request().method() === 'PATCH') {
      try {
        const body = JSON.parse(route.request().postData() ?? '{}') as { title?: string }
        if (body.title) promptTitle = body.title
      } catch { /* empty */ }
      return route.fulfill({
        json: { id: 'p1', scope: 'personal', title: promptTitle, body: 'Updated body', tags: ['infrastructure'], agent_id: 'auto' },
      })
    }
    if (url.includes('/ai/prompts') && route.request().method() === 'POST') {
      return route.fulfill({
        json: { id: 'p2', scope: 'personal', title: 'New prompt', body: 'Body', tags: [], agent_id: 'auto' },
      })
    }
    if (url.includes('/ai/prompts')) {
      return route.fulfill({
        json: [{ id: 'p1', scope: 'personal', title: promptTitle, body: 'Analyze host pressure', tags: ['infrastructure'], agent_id: 'auto' }],
      })
    }
    if (url.match(/\/ai\/providers\/[^/]+\/models/)) {
      return route.fulfill({
        json: [{ id: 'mod-1', provider_id: 'prov-1', model_id: 'gpt-4o-mini', display_name: 'gpt-4o-mini', context_window: 128000, enabled: true }],
      })
    }
    if (url.includes('/ai/providers')) {
      return route.fulfill({
        json: [{
          id: 'prov-1',
          name: 'OpenAI',
          kind: 'openai',
          base_url: '',
          org_id: '',
          deployment_name: '',
          api_key_configured: true,
          enabled: true,
          is_default: true,
        }],
      })
    }
    if (url.includes('/ai/marketplace/agents')) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/ai/autopilot/propose')) {
      return route.fulfill({
        json: {
          mode: 'advisor',
          actions: [
            {
              id: 'auto-1',
              label: 'Power off idle VM',
              review: 'idle-vm unused 30d',
              risk: 'low',
              action_type: 'vm.power_off',
              object_ref: { vm_id: 'v2' },
            },
          ],
        },
      })
    }
    if (url.includes('/ai/copilot/stream') && route.request().method() === 'POST') {
      const sse = [
        'data: {"type":"chunk","text":"Fleet looks healthy. "}',
        'data: {"type":"chunk","text":"2 VMs running."}',
        'data: {"type":"done","deterministic":true}',
      ].join('\n\n') + '\n\n'
      return route.fulfill({
        status: 200,
        headers: { 'Content-Type': 'text/event-stream' },
        body: sse,
      })
    }
    if (url.includes('/ai/cost') && !url.includes('/ai/cost/') && route.request().method() === 'GET') {
      return route.fulfill({
        json: {
          estimated_monthly_usd: 120,
          predicted_next_month_usd: 130,
          vm_count: 2,
          idle_vm_count: 1,
          oversized_vm_count: 0,
          snapshot_heavy_count: 0,
          suggestions: ['Power off idle-vm'],
        },
      })
    }
    if (url.includes('/ai/capacity') && !url.includes('/ai/capacity/export') && route.request().method() === 'GET') {
      return route.fulfill({
        json: {
          hosts_online: 1,
          memory_headroom_mib: 8192,
          avg_cpu_percent: 25,
          storage_used_gib: 40,
          storage_capacity_gib: 200,
          cpu_headroom_percent: 60,
          estimated_small_vms_addable: 4,
          recommendations: ['Headroom OK'],
        },
      })
    }
    if (url.includes('/ai/compliance') && !url.includes('/export') && route.request().method() === 'GET') {
      return route.fulfill({
        json: {
          score: 82,
          grade: 'B',
          summary: 'Fleet compliance OK',
          markdown: '# Compliance\n',
          checks: [{ id: 'c1', name: 'TLS', passed: true, detail: 'Controller TLS enabled' }],
        },
      })
    }
    if (url.includes('/ai/security') && route.request().method() === 'GET' && !url.includes('/ai/security/')) {
      return route.fulfill({
        json: {
          summary: 'No critical findings',
          findings: [{ id: 'f1', severity: 'low', title: 'Open SSH', detail: 'Port 22 exposed' }],
        },
      })
    }
    if (url.includes('/ai/cost/budget') && route.request().method() === 'GET') {
      return route.fulfill({
        json: {
          monthly_budget_usd: 500,
          current_spend_usd: 120,
          predicted_spend_usd: 140,
          utilization_pct: 24,
          status: 'ok',
          alerts: [],
        },
      })
    }
    if (url.includes('/ai/cost/attribution') && !url.includes('/export') && route.request().method() === 'GET') {
      return route.fulfill({
        json: {
          summary: '1 team',
          teams: [{ team: 'platform', vm_count: 2, estimated_monthly_usd: 120, share_pct: 100 }],
        },
      })
    }
    if (url.includes('/ai/autopilot/history')) {
      return route.fulfill({
        json: [
          { id: 'ap1', action: 'ai.autopilot.restart_agent', created_at: new Date().toISOString(), status: 'ok' },
        ],
      })
    }
    if (url.includes('/zeus-firewall/finops/exposure') && !url.includes('/export')) {
      return route.fulfill({
        json: {
          summary: '1 high-risk VM',
          fleet_exposure_monthly_usd: 240,
          idle_port_waste_usd: 45,
          cloud_sg_monthly_usd: 80,
          gpu_exposure_usd: 30,
          storage_exposure_usd: 20,
          mission_stack_network_usd: 10,
          public_port_alerts: [],
          targets: [],
          vm_idle_ranking: [{ vm_id: 'v1', vm_name: 'vm-1', rank: 1, waste_usd: 45, idle_ports: 2 }],
        },
      })
    }
    if (
      url.includes('/ai/cost/export.csv')
      || url.includes('/ai/capacity/export.csv')
      || url.includes('/ai/cost/attribution/export.csv')
      || url.includes('/ai/compliance/export')
    ) {
      const isPdf = url.includes('.pdf')
      return route.fulfill({
        body: isPdf ? '%PDF-1.4 mock' : 'metric,value\nidle_vms,1\n',
        contentType: isPdf ? 'application/pdf' : 'text/csv',
      })
    }
    if (url.includes('/ai/nl-ops')) {
      return route.fulfill({
        json: { intent: 'search', summary: '1 result', steps: [], risk_score: 1, dry_run: true, approval_required: false, action_ids: [], reply: 'Found 1 VM.' },
      })
    }
    if (url.includes('/ai/mission/stack/status')) {
      return route.fulfill({ json: { summary: 'No active mission stack', status: 'idle' } })
    }
    if (url.includes('/enterprise/security/overview')) {
      return route.fulfill({ json: enterpriseSecurity })
    }
    if (url.match(/\/enterprise\/tenants\/policies\/[^/]+$/) && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          project_name: 'default',
          vm_count: 2,
          max_vms: 50,
          network_isolation: 'shared',
          enforce_quotas: true,
          quota_status: 'ok',
        },
      })
    }
    if (url.includes('/enterprise/fips/matrix')) {
      return route.fulfill({ json: enterpriseFips })
    }
    if (url.includes('/enterprise/tenants/overview')) {
      return route.fulfill({ json: enterpriseTenants })
    }
    if (url.match(/\/fleet\/keychain(\?|$|\/)/)) {
      return route.fulfill({ json: fleetKeychain })
    }
    if (url.includes('/fleet/updates')) {
      return route.fulfill({ json: fleetUpdates })
    }
    if (url.includes('/upgrade/matrix')) {
      return route.fulfill({
        json: {
          controller_version: '0.1.0-test',
          recommended_agent: '0.1.0-test',
          min_agent: '0.0.9',
          notes: 'Agents below minimum lose guest health and enforcement RPCs.',
        },
      })
    }
    if (url.includes('/maintenance/schedules') && route.request().method() === 'POST') {
      if (url.includes('fail-schedule')) {
        return route.fulfill({ status: 500, json: { error: 'schedule failed' } })
      }
      return route.fulfill({
        json: {
          id: 'sched-1',
          host_id: 'h1',
          action: 'enter',
          evacuate: true,
          run_at: new Date().toISOString(),
          status: 'pending',
        },
      })
    }
    if (url.includes('/maintenance/schedules')) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/hosts/') && url.includes('/maintenance') && route.request().method() === 'POST') {
      return route.fulfill({ json: { task_id: 'task-maint-1' } })
    }
    if (url.includes('/ai/fleet/gpu-placement')) {
      return route.fulfill({
        json: {
          summary: '1 GPU-capable host(s) for inference — top: host-1',
          candidates: [{ host_id: 'h1', hostname: 'host-1', gpu_capable: true, numa_hint: 'NUMA 0', score: 92, reason: 'GPU-tagged host · CPU 35% · 1 VMs' }],
        },
      })
    }
    if (url.includes('/ai/fleet/heatmap')) {
      return route.fulfill({
        json: {
          hosts: [{ host_id: 'h1', hostname: 'host-1', cpu_percent: 35, memory_percent: 40, vm_count: 1, classification: 'balanced' }],
          hotspots: [],
          cold_hosts: [],
          power_waste_hosts: [],
        },
      })
    }
    if (url.includes('/ai/fleet/rebalance/execute') && route.request().method() === 'POST') {
      const body = route.request().postDataJSON() as { dry_run?: boolean } | null
      const moves = [{ vm_id: 'v1', vm_name: 'vm-1', from_host: 'host-1', to_host: 'host-2', reason: 'relieve hotspot' }]
      if (body?.dry_run === false) {
        return route.fulfill({
          json: {
            dry_run: false,
            task_ids: ['task-migrate-abc123def456'],
            moves,
            summary: 'Queued 1 live migration',
          },
        })
      }
      return route.fulfill({
        json: { dry_run: true, task_ids: [], moves, summary: 'Preview: 1 move(s) would be queued' },
      })
    }
    if (url.includes('/ai/fleet/rebalance')) {
      return route.fulfill({
        json: {
          summary: '1 move suggested',
          moves: [{ vm_id: 'v1', vm_name: 'vm-1', from_host: 'host-1', to_host: 'host-2', reason: 'relieve hotspot' }],
          estimated_savings_pct: 12,
        },
      })
    }
    if (url.includes('/fleet/linux-health')) {
      return route.fulfill({
        json: {
          hosts_scanned: 1,
          pressure_hosts: 1,
          thermal_alerts: 0,
          smart_alerts: 0,
          summary: '1 host(s) scanned · 1 under pressure · 0 thermal · 0 SMART',
          hosts: [{ host_id: 'h1', hostname: 'host-1', io_pressure_pct: 55, thermal_max_c: 0, smart_failures: 0, status: 'pressure' }],
        },
      })
    }
    if (url.includes('/fleet/activity')) {
      return route.fulfill({ json: fleetActivity })
    }
    if (url.includes('/ai/fleet/power')) {
      return route.fulfill({
        json: { summary: 'No power waste detected', total_savings_usd_month: 0, optimizations: [] },
      })
    }
    if (url.includes('/ai/fleet/summary')) {
      return route.fulfill({ json: { summary: '1 host · 1 VM', hosts: 1, vms: 1, alerts: [] } })
    }
    if (url.includes('/ai/fleet/local')) {
      return route.fulfill({ json: { summary: 'Local agent OK', local_agent: {} } })
    }
    if (url.includes('/operations/showback')) {
      return route.fulfill({ json: opsShowback })
    }
    if (url.includes('/operations/overview')) {
      return route.fulfill({
        json: {
          runbook_count: opts?.emptyRunbooks ? 0 : 1,
          executions_24h: 0,
          compliance_grade: 'B+',
        },
      })
    }
    if (url.includes('/operations/executions')) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/operations/runbooks') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          execution_id: 'ex-1',
          incident: 'host-offline',
          title: 'Host offline recovery',
          steps: ['Verify host heartbeat', 'Restart libvirtd if needed'],
          commands: ['systemctl status libvirtd'],
          summary: 'Runbook steps recorded',
        },
      })
    }
    if (url.includes('/operations/runbooks')) {
      return route.fulfill({ json: opts?.emptyRunbooks ? [] : opsRunbooks })
    }
    if (url.includes('/reports/capacity')) {
      return route.fulfill({
        json: {
          hosts_online: 1,
          hosts_offline: 0,
          total_vms: 1,
          running_vms: 1,
          memory_total_mib: 16384,
          memory_used_mib: 4096,
          memory_headroom_mib: 8192,
          avg_cpu_percent: 35,
          planner_recommendations: [],
        },
      })
    }
    if (url.includes('/reports/finops')) {
      return route.fulfill({
        json: {
          estimated_monthly_usd: 100,
          total_vcpu: 4,
          vcpu_hour_usd: 0.01,
          total_memory_gib: 8,
          gib_hour_usd: 0.005,
          vm_count: 1,
          running_vms: 1,
        },
      })
    }
    if (url.includes('/projects')) {
      return route.fulfill({ json: [{ name: 'default', vm_count: 1 }] })
    }
    if (url.includes('/fleet/desktop')) {
      return route.fulfill({ json: fleetDesktop })
    }
    if (url.includes('/policy/rules')) {
      return route.fulfill({ json: [{ id: '1', name: 'default', enabled: true, rule_json: {} }] })
    }
    if (url.includes('/policy/quotas')) {
      return route.fulfill({
        json: [{ project: 'default', max_vms: 50, max_vcpu: 200, max_memory_mib: 409600, max_storage_gib: 5000 }],
      })
    }
    if (url.includes('/audit') && !url.includes('/linux/audit')) {
      return route.fulfill({ json: [{ id: 'a1', actor: 'admin', action: 'login', created_at: new Date().toISOString() }] })
    }
    if (url.includes('/observability/overview')) {
      return route.fulfill({
        json: {
          summary: 'SLOs OK',
          slos: [{
            name: 'API latency',
            target: 'p99 < 500ms',
            objective_pct: 99,
            current_pct: 99.2,
            burn_rate: 0.01,
            status: 'ok',
            description: 'Within SLO',
          }],
          trace_count_1h: 12,
          p95_latency_ms: 45,
        },
      })
    }
    if (url.includes('/observability/traces')) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/placement/recommendations') || url.includes('/placement/refresh')) {
      return route.fulfill({ json: opts?.placementRecommendations ? [placementRecommendation] : [] })
    }
    if (url.includes('/ha/status')) {
      return route.fulfill({
        json: {
          status: { enabled_vms: 0, offline_hosts: 0, recent_events: 0 },
          events: [],
        },
      })
    }
    if (url.includes('/cluster/settings')) {
      return route.fulfill({
        json: {
          drs_auto_migrate: false,
          drs_cpu_threshold: 80,
          ha_enabled: true,
          placement_policy: 'balanced',
          inventory_sync_interval_secs: 60,
        },
      })
    }
    if (url.includes('/migrations/advisor')) {
      return route.fulfill({
        json: {
          vm_name: 'vcenter-vm-1',
          provider: 'vmware',
          readiness_percent: 85,
          safe: ['virtio drivers present'],
          risks: [],
          recommended_target: { hypervisor: 'kvm' },
          remediation: [],
          guestkit_migration_score: 82,
          guestkit_summary: 'KVM migration feasible',
        },
      })
    }
    if (url.match(/\/api\/v1\/migrations(\?|$)/)) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/fence/events')) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/storage/pools/discover')) {
      storagePools = [{ id: 'p1', name: 'default', path: '/var/lib/libvirt/images', capacity_gib: 500, used_gib: 12 }]
      return route.fulfill({ json: { imported: 1, pools: storagePools } })
    }
    if (url.includes('/networks/live')) {
      return route.fulfill({
        json: {
          host_id: 'h1',
          networks: [
            { name: 'default', active: true, persistent: true, autostart: true, bridge: 'virbr0' },
            { name: 'isolated', active: false, persistent: true, autostart: false, bridge: 'virbr1' },
          ],
        },
      })
    }
    if (url.includes('/networks/discover')) {
      const nets = opts?.emptyNetworks ? [] : platformNetworks
      return route.fulfill({ json: { imported: nets.length, networks: nets } })
    }
    if (url.match(/\/networks\/[^/]+$/) && route.request().method() === 'PATCH') {
      const id = url.split('/').pop() ?? ''
      const body = (route.request().postDataJSON() ?? {}) as { bridge?: string; vlan_id?: number }
      const idx = platformNetworks.findIndex((n) => n.id === id)
      if (idx >= 0) {
        platformNetworks[idx] = {
          ...platformNetworks[idx],
          bridge: body.bridge ?? platformNetworks[idx].bridge,
          vlan_id: body.vlan_id ?? platformNetworks[idx].vlan_id,
        }
        return route.fulfill({ json: platformNetworks[idx] })
      }
      return route.fulfill({ status: 404, json: { error: 'network not found' } })
    }
    if (url.includes('/networks') && route.request().method() === 'POST') {
      return route.fulfill({ json: { id: 'n2', name: 'vm-net', bridge: 'br0', backend: 'bridge' } })
    }
    if (url.includes('/storage/pools/live')) {
      return route.fulfill({
        json: {
          host_id: 'h1',
          pools: [
            { name: 'default', state: 'active', path: '/var/lib/libvirt/images', capacity_gb: 500, allocation_gb: 80, available_gb: 420, autostart: true },
            { name: 'data', state: 'inactive', path: '/data/libvirt', capacity_gb: 1000, allocation_gb: 100, available_gb: 900, autostart: false },
          ],
        },
      })
    }
    if (url.includes('/storage/pools/') && url.includes('/snapshot-policy')) {
      return route.fulfill({
        json: {
          pool_name: 'default',
          snapshot_retention_days: 14,
          summary: 'Tier silver · 14-day libvirt snapshot retention',
        },
      })
    }
    if (url.includes('/storage/pools') && route.request().method() === 'POST') {
      return route.fulfill({
        json: { id: 'p2', name: 'datastore-01', path: '/var/lib/libvirt/images', capacity_gib: 500, used_gib: 0, backend: 'directory' },
      })
    }
    if (url.match(/\/storage\/pools\/[^/]+\/volumes\/[^/?]+/) && route.request().method() === 'DELETE') {
      return route.fulfill({ json: { deleted: true } })
    }
    if (url.match(/\/storage\/pools\/[^/]+\/volumes/) && route.request().method() === 'POST') {
      return route.fulfill({ json: { task_id: 'task-vol-create-mock', status: 'pending' } })
    }
    if (url.match(/\/storage\/pools\/[^/]+\/volumes/)) {
      return route.fulfill({
        json: {
          pool: 'default',
          volumes: [{ name: 'vol-a', type: 'file', path: '/var/lib/libvirt/images/vol-a.qcow2', capacity_bytes: 10_737_418_240 }],
        },
      })
    }
    if (url.includes('/storage/pools') && !url.includes('/discover')) {
      // Both the platform/controller-routed StoragePool[] (api/platformStorage.ts, gib-shaped) and
      // the classic daemon StoragePoolInfo[] (api/storage.ts, gb-shaped) hit a URL containing
      // '/storage/pools' — the platform one is routed through PLATFORM_CONTROLLER_PROXY, so that
      // substring in the URL is what tells the two apart.
      return route.fulfill({ json: url.includes('/platform/controller/') ? storagePools : classicStoragePools })
    }
    if (url.includes('/enrollment/tokens') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          token: 'enroll-test-token',
          expires_at: new Date(Date.now() + 86400000).toISOString(),
          install_command: 'curl -fsSL https://example/install.sh | sudo bash -s enroll-test-token',
        },
      })
    }
    if (url.includes('/storage/tiers')) {
      return route.fulfill({ json: { tiers: [], pools: [] } })
    }
    if (url.includes('/network/segments/overview')) {
      return route.fulfill({ json: { segments: [], summary: 'No segments' } })
    }
    if (url.includes('/storage/backup-sla')) {
      return route.fulfill({ json: { policies: [], summary: 'No SLA configured' } })
    }
    if (url.includes('/fleet/storage')) {
      return route.fulfill({
        json: {
          summary: 'OK',
          pool_count: 2,
          tier_count: 0,
          total_capacity_gib: 500,
          total_used_gib: 120,
          pools_over_85_pct: 0,
          smart_failure_count: 0,
          smart_hosts_affected: 0,
          pools: [
            { id: 'p1', name: 'default', storage_class: 'local', used_gib: 80, capacity_gib: 250, used_pct: 32 },
            { id: 'p2', name: 'data', storage_class: 'local', used_gib: 40, capacity_gib: 250, used_pct: 16 },
          ],
          smart_disks: [],
        },
      })
    }
    if (url.includes('/fleet/network')) {
      return route.fulfill({
        json: {
          summary: '2 networks',
          network_count: 2,
          hosts_online: 1,
          segments: [],
        },
      })
    }
    if (url.includes('/soc/')) {
      if (url.includes('/overview')) {
        return route.fulfill({ json: { open_alerts: 2, events_24h: 48, critical_alerts: 1 } })
      }
      if (url.includes('/events')) {
        return route.fulfill({
          json: [
            {
              id: 'e1',
              occurred_at: new Date().toISOString(),
              source: 'machina-bpf',
              category: 'intrusion_detection',
              severity: 'high',
              summary: 'Unusual port activity on db-01',
            },
          ],
        })
      }
      if (url.includes('/alerts')) {
        const alertIdMatch = url.match(/\/alerts\/([^/?]+)$/)
        if (alertIdMatch && route.request().method() === 'GET') {
          return route.fulfill({
            json: {
              id: 'a1',
              rule_id: 'r1',
              rule_name: 'critical_anomaly',
              title: 'critical_anomaly (1)',
              severity: 'high',
              status: 'open',
              assigned_to: null,
              first_seen: new Date().toISOString(),
              last_seen: new Date().toISOString(),
              event_count: 1,
              dedupe_key: 'rule:r1:2026-06-03-03',
              detail_json: { rule: 'critical_anomaly', match_count: 1 },
              mitre_tags: [{ id: 'T1046', name: 'Network Service Discovery' }],
              linked_events: [
                {
                  id: 'e1',
                  occurred_at: new Date().toISOString(),
                  source: 'machina-bpf',
                  category: 'intrusion_detection',
                  severity: 'high',
                  summary: 'Unusual port activity on db-01',
                  ecs_json: { 'threat': { technique: { id: 'T1046', name: 'Network Service Discovery' } } },
                },
              ],
              playbook_runs: [],
            },
          })
        }
        if (alertIdMatch && route.request().method() === 'PATCH') {
          return route.fulfill({
            json: {
              id: 'a1',
              title: 'critical_anomaly (1)',
              severity: 'high',
              status: 'acknowledged',
              first_seen: new Date().toISOString(),
              last_seen: new Date().toISOString(),
              event_count: 1,
            },
          })
        }
        return route.fulfill({
          json: [
            {
              id: 'a1',
              title: 'critical_anomaly (1)',
              severity: 'high',
              status: 'open',
              first_seen: new Date().toISOString(),
              last_seen: new Date().toISOString(),
              event_count: 1,
            },
          ],
        })
      }
      if (url.includes('/rules')) {
        if (url.includes('/test')) {
          return route.fulfill({ json: { match_count: 3, would_fire: true } })
        }
        return route.fulfill({
          json: [
            {
              id: 'r1',
              name: 'critical_anomaly',
              description: 'eBPF critical or high severity anomaly',
              enabled: true,
              severity: 'high',
              query_json: { type: 'match' },
              throttle_minutes: 30,
              builtin: true,
            },
          ],
        })
      }
      if (url.includes('/asm/summary')) {
        return route.fulfill({
          json: {
            exposure_score: 72,
            firewall_targets: 2,
            high_risk_nodes: 1,
            open_port_findings: [{ kind: 'firewall_target', resource: 'host-1', detail: 'Risk high score 85', severity: 'high' }],
            recommendations: ['Review Zeus Firewall open ports'],
          },
        })
      }
      if (url.includes('/integrations') && !url.includes('/integrations/splunk')) {
        if (url.match(/\/integrations\/[^/]+\/test/)) {
          return route.fulfill({ json: { ok: true, message: 'Integration test OK' } })
        }
        if (route.request().method() === 'PATCH') {
          return route.fulfill({
            json: {
              id: 'int-1',
              integration_type: 'elastic_bulk',
              name: 'default',
              enabled: true,
              config: { url: 'https://elastic:9200', api_key: '••••••••' },
              last_success_at: new Date().toISOString(),
            },
          })
        }
        return route.fulfill({
          json: [
            {
              id: 'splunk-1',
              integration_type: 'splunk_hec',
              name: 'default',
              enabled: false,
              config: { url: '', index: 'machina' },
            },
            {
              id: 'elastic-1',
              integration_type: 'elastic_bulk',
              name: 'default',
              enabled: false,
              config: { url: '', index: 'logs-machina.soc' },
            },
          ],
        })
      }
      if (url.includes('/soc/settings')) {
        if (route.request().method() === 'PATCH') {
          return route.fulfill({ json: { webhook_url: 'https://hooks.example/soc' } })
        }
        return route.fulfill({ json: { webhook_url: '' } })
      }
      if (url.includes('/playbook-runs')) {
        return route.fulfill({ json: [] })
      }
      if (url.includes('/soc/playbooks')) {
        const pb = {
          id: 'pb1',
          name: 'notify_on_critical',
          description: 'Webhook notify when critical SOC alert opens',
          enabled: true,
          trigger_json: { min_severity: 'high', rule_names: [] },
          steps_json: [
            { type: 'webhook', url_from_setting: 'soc_webhook_url', body: { alert_id: '{{alert_id}}' } },
          ],
        }
        if (route.request().method() === 'POST') {
          return route.fulfill({ json: { ...pb, id: 'pb2', name: 'new_playbook' } })
        }
        if (url.match(/\/playbooks\/[^/]+$/)) {
          return route.fulfill({ json: pb })
        }
        return route.fulfill({ json: [pb] })
      }
      if (url.includes('/ingest/run')) {
        return route.fulfill({
          json: {
            ingest: { firewall: 1, audit: 2, platform: 0, native_bpf: 1 },
            alerts_fired: 0,
            forwarded: 2,
          },
        })
      }
      if (url.includes('/integrations/splunk')) {
        if (url.includes('/test')) {
          return route.fulfill({ json: { ok: true, message: 'Splunk HEC accepted test event' } })
        }
        if (route.request().method() === 'PUT') {
          return route.fulfill({
            json: {
              id: 'splunk-1',
              integration_type: 'splunk_hec',
              name: 'default',
              enabled: true,
              config: { url: 'https://splunk:8088', token: '••••••••', index: 'machina' },
            },
          })
        }
        return route.fulfill({
          json: {
            id: 'splunk-1',
            integration_type: 'splunk_hec',
            name: 'default',
            enabled: false,
            config: { url: '', token: '', index: 'machina' },
          },
        })
      }
      if (url.includes('/forward/replay')) {
        return route.fulfill({ json: { forwarded: 12, hours: 24 } })
      }
      return route.fulfill({ json: [] })
    }
    if (url.match(/\/zeus-security\/hosts\/[^/]+\/enforcement/)) {
      return route.fulfill({
        json: {
          mode: 'enforce',
          summary: '1 policy applied on host-1',
          policies: [{ id: 'pol-deny-nc', name: 'Block reverse-shell listeners', kind: 'deny_process', match: '/usr/bin/nc' }],
        },
      })
    }
    if (url.includes('/zeus-security/')) {
      if (url.includes('/status')) {
        return route.fulfill({
          json: {
            fabric_reachable: true,
            native_bpf: { enabled: true, reachable: true, hosts_total: 2, hosts_reachable: 2, enforcing_hosts: 0, version: '0.1.0', summary: 'machina-bpfd healthy on 2/2 host(s)' },
            zeus_firewall: { ready: true },
          },
        })
      }
      if (url.includes('/fleet/threat')) {
        return route.fulfill({
          json: {
            fleet_threat_score: 78,
            firewall_targets: 1,
            critical_events: [{ summary: 'Possible reverse shell on port 4444', host_id: 'h1', severity: 'critical' }],
            native_bpf: { fleet_threat_score: 78 },
            security_graph_summary: '4 nodes · 3 edges in infrastructure security graph',
          },
        })
      }
      if (url.includes('/fleet/timeline')) {
        return route.fulfill({
          json: {
            events: [
              { summary: 'curl started', host_id: 'h1', severity: 'info', timestamp: new Date().toISOString() },
              { summary: 'Privilege escalation chain', host_id: 'h1', severity: 'high', kind: 'correlation' },
            ],
          },
        })
      }
      if (url.includes('/fabric/health')) {
        return route.fulfill({
          json: {
            status: 'healthy',
            sensors_total: 1,
            sensors_healthy: 1,
            summary: '1 sensor(s) · 128 OpenSearch doc(s)',
            hunt_index: { configured: true, reachable: true, document_count: 128 },
            issues: [],
          },
        })
      }
      if (url.includes('/hunt/queries')) {
        return route.fulfill({
          json: {
            queries: [
              { id: 'shell-spawn', name: 'Reverse shell listeners', query: 'nc OR netcat', severity: 'critical' },
            ],
          },
        })
      }
      if (url.includes('/hunt/run/')) {
        return route.fulfill({
          json: {
            ok: true,
            query_id: 'shell-spawn',
            query_name: 'Reverse shell listeners',
            backend: 'opensearch',
            hit_count: 1,
            results: [{ summary: 'nc listener on 4444', host_id: 'h1', severity: 'critical' }],
          },
        })
      }
      if (url.includes('/search') && route.request().method() === 'POST') {
        return route.fulfill({
          json: {
            results: [
              { summary: 'nc listener on port 4444', host_id: 'h1', severity: 'critical', kind: 'process' },
            ],
          },
        })
      }
      if (url.includes('/correlations')) {
        return route.fulfill({
          json: { correlations: [{ severity: 'high', summary: 'Suspicious DNS cluster', host_id: 'h2' }] },
        })
      }
      if (url.includes('/enforcement/status')) {
        return route.fulfill({
          json: {
            mode: 'enforce',
            policies_total: 4,
            policies_enabled: 4,
            applied_hosts: ['h1'],
            blocked_events: 2,
            summary: '4 active policy(ies) · 2 blocked event(s) in store',
          },
        })
      }
      if (url.includes('/fleet/sensors')) {
        return route.fulfill({
          json: {
            summary: '2 host(s) · 1 healthy machina-bpfd',
            sensors: [{ host_id: 'h1', status: 'healthy', version: '0.1.0', kind: 'machina-bpf' }],
            matrix: [
              { host_id: 'h1', hostname: 'host-1', host_state: 'online', sensor_status: 'healthy' },
              { host_id: 'h2', hostname: 'host-2', host_state: 'online', sensor_status: 'unreachable' },
            ],
          },
        })
      }
      if (url.match(/\/enforcement\/policies\/[^/]+\/document/)) {
        return route.fulfill({
          json: {
            policy_id: 'pol-deny-nc',
            native_policy: { id: 'ctl-pol-deny-nc', kind: 'deny_process', match: '/usr/bin/nc', scope: { kind: 'host' } },
            datapath_rules: ['exec deny path=/usr/bin/nc'],
            api_mode: 'native',
          },
        })
      }
      if (url.match(/\/enforcement\/policies\/[^/]+\/apply/)) {
        return route.fulfill({
          json: {
            native: { ok: true, sync: [{ host_id: 'h1', hostname: 'host-1', ok: true }] },
            summary: 'Applied Block reverse-shell listeners to 1 host(s)',
          },
        })
      }
      if (url.match(/\/enforcement\/policies\/[^/]+/) && route.request().method() === 'PATCH') {
        return route.fulfill({
          json: { native: { ok: true, sync: [] }, summary: 'Enforcement policy pol-deny-nc updated' },
        })
      }
      if (url.match(/\/enforcement\/policies\/[^/]+/) && route.request().method() === 'DELETE') {
        return route.fulfill({
          json: { native: { ok: true, sync: [] }, summary: 'Enforcement policy pol-deny-nc deleted' },
        })
      }
      if (url.includes('/enforcement/policies') && route.request().method() === 'POST') {
        return route.fulfill({
          json: {
            native: { ok: true, policy: { id: 'pol-new', name: 'test', kind: 'deny_process', match: '/bin/sh', enabled: true } },
            summary: 'Policy test created',
          },
        })
      }
      if (url.includes('/enforcement/policies')) {
        return route.fulfill({
          json: {
            policies: [
              { id: 'pol-deny-nc', name: 'Block reverse-shell listeners', kind: 'deny_process', match: '/usr/bin/nc', enabled: true, scope: 'fleet' },
              { id: 'pol-deny-shadow', name: 'Block shadow file read', kind: 'deny_file', match: '/etc/shadow', enabled: true },
              { id: 'pol-deny-raw', name: 'Block raw socket capability', kind: 'deny_cap', match: 'CAP_NET_RAW', enabled: true },
            ],
          },
        })
      }
      if (url.includes('/enforcement')) {
        return route.fulfill({ json: { mode: 'observe', policies: [] } })
      }
      if (url.includes('/alerts/sync')) {
        return route.fulfill({ json: { inserted: 1, summary: 'Synced 1 security alert(s) to notification outbox' } })
      }
      if (url.includes('/graph')) {
        return route.fulfill({
          json: {
            nodes: [
              { id: 'user-1', kind: 'user', label: 'admin', risk: 'high' },
              { id: 'host-h1', kind: 'host', label: 'host-1', risk: 'medium' },
            ],
            edges: [{ from: 'user-1', to: 'host-h1', label: 'admin access' }],
          },
        })
      }
      if (url.includes('/sensors')) {
        return route.fulfill({ json: { sensors: [{ host_id: 'h1', status: 'healthy', version: '0.1.0', kind: 'machina-bpf' }] } })
      }
      if (url.includes('/process-graph')) {
        return route.fulfill({
          json: {
            nodes: [{ pid: 1000, binary: '/usr/sbin/sshd' }, { pid: 1234, binary: '/bin/bash' }],
            edges: [{ from: 1000, to: 1234, binary: '/bin/bash' }],
          },
        })
      }
      if (url.includes('/timeline') || url.includes('/processes')) {
        return route.fulfill({
          json: {
            events: [{ summary: 'curl started', kind: 'process_exec', severity: 'info', timestamp: new Date().toISOString() }],
            processes: [{ summary: 'kubectl started', kind: 'process_exec', process: { binary: '/usr/bin/kubectl', pid: 1235 } }],
          },
        })
      }
      if (url.includes('/fabric-status')) {
        return route.fulfill({
          json: {
            host_id: 'h1',
            agent_reachable: true,
            fabric: {
              available: true,
              programs_compiled: true,
              version: '0.1.0',
              mode: { mode: 'observe' },
              policies_total: 1,
              policies_enabled: 1,
              interfaces: [{ name: 'vnet0', ifindex: 7, vm: 'web-1', scope: 1, flags: ['net'], guest_side: false, xdp: false, qos_egress_bps: 0, qos_ingress_bps: 0 }],
            },
          },
        })
      }
      if (url.includes('/summary')) {
        return route.fulfill({ json: { host_id: 'h1', threat_score: 75, sensor: { status: 'healthy' } } })
      }
      if (url.includes('/containers')) {
        return route.fulfill({
          json: {
            host_id: 'h1',
            summary: '2 namespace(s) with pod/container metadata from machina-bpfd',
            namespaces: [
              {
                namespace: 'zeus',
                event_count: 4,
                pods: [
                  {
                    pod: 'api-server-7f8c9',
                    deployment: 'api-server',
                    containers: [{ container: 'api', event_count: 3, max_severity: 'info', processes: ['/app/server'] }],
                  },
                ],
              },
            ],
          },
        })
      }
      if (url.includes('/asset-inventory')) {
        return route.fulfill({
          json: { hosts: [{ host_id: 'h1', processes: ['nginx', 'postgres'], connections: [{ from: 'nginx', to: 'redis:6379' }] }] },
        })
      }
      return route.fulfill({ json: { ports: [], connections: [], dns: [], files: [] } })
    }
    if (url.includes('/fleet/') && !url.match(/\/fleet\/(finder|mission|gpu|desktop|storage|network|console|updates|keychain|users|shortcuts|spaces|general|linux|backups|activity|dna|maintenance-mission)/)) {
      return route.fulfill({ json: { summary: 'Fleet aggregate OK', hosts: [], entries: [] } })
    }
    if (url.includes('/cluster')) {
      return route.fulfill({ json: { name: 'e2e-cluster', hosts: 1, vms: 2, offline_hosts: 0 } })
    }
    if (url.includes('/templates/prefetch-missing')) {
      return route.fulfill({ json: { task_id: 'prefetch-task-1' } })
    }
    if (url.includes('/templates/missing-images')) {
      return route.fulfill({
        json: {
          missing: [
            { name: 'photon-os', version: '1.0.0', source_disk: '/var/lib/libvirt/images/photon-os.qcow2', category: 'Appliance', icon: '📦', auto_fetch: true },
          ],
          count: 1,
          auto_fetch_count: 1,
          summary: '1 golden image(s) missing — 1 can auto-download on first VM create.',
        },
      })
    }
    if (url.includes('/templates/') && url.includes('/readiness')) {
      if (opts?.templateNotReady) {
        return route.fulfill({
          json: {
            disk_exists: false,
            host_online: 1,
            cloud_init: true,
            ready: false,
            auto_fetch: false,
            remediation: 'Upload the golden image to Content Library.',
            source_disk: sampleTemplate.source_disk,
          },
        })
      }
      if (opts?.templateAutoFetch) {
        templateReadinessPolls += 1
        const fetched = templateReadinessPolls > 1
        return route.fulfill({
          json: {
            disk_exists: fetched,
            host_online: 1,
            cloud_init: true,
            ready: true,
            auto_fetch: true,
            remediation: fetched ? 'Disk present on 1 online host(s).' : 'Golden image will download over SSH on first create.',
            source_disk: sampleTemplate.source_disk,
          },
        })
      }
      return route.fulfill({
        json: {
          disk_exists: true,
          host_online: 1,
          cloud_init: true,
          ready: true,
          auto_fetch: false,
          remediation: 'Disk present on 1 online host(s).',
          source_disk: sampleTemplate.source_disk,
        },
      })
    }
    if (url.includes('/templates/sync-git/webhook') && route.request().method() === 'POST') {
      return route.fulfill({ json: { synced: 2, source: 'webhook' } })
    }
    if (url.includes('/templates/sync-git') && route.request().method() === 'POST') {
      return route.fulfill({ json: { synced: 1 } })
    }
    if (url.includes('/templates/seed')) {
      return route.fulfill({ json: { inserted: 1, templates: [sampleTemplate] } })
    }
    if (url.includes('/templates/marketplace')) {
      return route.fulfill({ json: [sampleTemplate] })
    }
    if (url.includes('/templates')) {
      return route.fulfill({ json: [sampleTemplate, fleetOnlyTemplate] })
    }
    if (url.includes('/vms/prune-missing') && route.request().method() === 'POST') {
      return route.fulfill({ json: { deleted: 2 } })
    }
    if (url.match(/\/guestkit\/status/)) {
      return route.fulfill({
        json: {
          enabled: true,
          base_url: 'http://127.0.0.1:8790',
          insecure_tls: true,
          reachable: true,
          library_version: '0.1.0-test',
          worker_url: 'http://127.0.0.1:8790',
          worker_reachable: true,
          summary: 'GuestKit ready',
        },
      })
    }
    if (url.match(/\/guestkit\/vms\/[^/]+\/migrate-plan/)) {
      return route.fulfill({ json: guestkitMigratePlan })
    }
    if (url.match(/\/guestkit\/vms\/[^/]+\/doctor/)) {
      return route.fulfill({
        json: {
          image_path: '/var/lib/libvirt/images/vm-1.qcow2',
          target: 'kvm',
          boot_score: 88,
          confidence: 0.9,
          summary: 'Boot assurance OK',
          blockers: [],
          warnings: [],
          checks_passed: 4,
          checks_total: 4,
        },
      })
    }
    if (url.match(/\/hosts\/[^/]+\/gpus/)) {
      return route.fulfill({ json: hostGpus })
    }
    if (url.match(/\/hosts\/[^/]+\/detail/)) {
      return route.fulfill({ json: hostDetail })
    }
    if (url.match(/\/hosts\/[^/]+$/) && route.request().method() === 'PATCH') {
      const body = (route.request().postDataJSON() ?? {}) as Record<string, unknown>
      hostDetail = { ...hostDetail, ...body }
      return route.fulfill({ json: hostDetail })
    }
    if (url.match(/\/hosts\/[^/]+\/health-check/) && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          ok: true,
          checks: [
            { name: 'agent_ping', passed: true, message: 'QEMU guest agent reachable' },
            { name: 'libvirt', passed: true, message: 'libvirtd active' },
            { name: 'disk_pressure', passed: false, message: 'Root FS above 85%', remediation: 'Prune old VM snapshots' },
          ],
        },
      })
    }
    if (url.match(/\/hosts\/[^/]+\/validate$/) && route.request().method() === 'GET') {
      return route.fulfill({
        json: {
          ok: true,
          checks: [
            { name: 'tls_cert', passed: true, message: 'Agent certificate valid' },
            { name: 'clock_skew', passed: true, message: 'Clock skew under 2s' },
          ],
        },
      })
    }
    if (url.match(/\/hosts\/[^/]+$/) && route.request().method() === 'DELETE') {
      return route.fulfill({ json: { deleted: true } })
    }
    if (url.match(/\/hosts\/[^/]+\/libvirt(\?|$)/)) {
      const action = new URL(url).searchParams.get('action') ?? ''
      if (action === 'host.node_devices') {
        return route.fulfill({
          json: [
            {
              name: 'pci_0000_06_00_0',
              parent: 'computer',
              driver: 'vfio-pci',
              capability_type: 'pci',
              xml: '<device><name>pci_0000_06_00_0</name><capability type="pci"><address domain="0x0000" bus="0x06" slot="0x00" function="0x0"/></capability></device>',
            },
            {
              name: 'usb_1_2',
              parent: 'usb_1',
              driver: 'usb',
              capability_type: 'usb_device',
              xml: '<device><name>usb_1_2</name><capability type="usb_device"><vendor id="0x046d"/><product id="0xc52b"/></capability></device>',
            },
          ],
        })
      }
      if (action === 'host.pci') {
        return route.fulfill({
          json: [
            {
              slot: '06:00.0',
              class: 'VGA compatible controller',
              vendor: 'NVIDIA Corporation',
              device: 'GA102',
              iommu_group: '14',
            },
          ],
        })
      }
      if (route.request().method() === 'POST') {
        return route.fulfill({ json: { ok: true, task_id: 'task-host-libvirt-mock' } })
      }
    }
    if (url.includes('/hosts') && !url.includes('/libvirt')) {
      if (opts?.emptyStorage) {
        return route.fulfill({ json: [{ ...sampleHost, state: 'offline' }] })
      }
      const hosts = opts?.staleHost ? [listHost, staleHost] : [listHost]
      return route.fulfill({ json: hosts })
    }
    if (url.match(/\/api\/v1\/topology(\?|$)/)) {
      return route.fulfill({
        json: {
          nodes: [
            { id: 'h1', name: 'host-1', kind: 'host', state: 'online' },
            { id: 'sw1', name: 'tor-1', kind: 'switch', state: 'up' },
          ],
          edges: [{ from: 'h1', to: 'sw1', label: 'uplink' }],
          warnings: [],
        },
      })
    }
    if (url.includes('/ai/twin/graph')) {
      return route.fulfill({
        json: {
          nodes: [
            { id: 'h1', name: 'host-1', kind: 'host', state: 'online' },
            { id: 'vm1', name: 'vm-1', kind: 'vm', state: 'running' },
          ],
          edges: [{ from: 'vm1', to: 'h1', label: 'runs_on' }],
          node_count: 2,
          edge_count: 1,
        },
      })
    }
    if (url.includes('/ai/twin/simulate') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          results: [
            {
              action: 'shutdown',
              target: 'host-1',
              severity: 'high',
              summary: 'Batch: host shutdown affects 1 VM',
              affected_vms: ['vm-1'],
              affected_applications: ['web-tier'],
              storage_risks: [],
              network_notes: [],
              recommendations: ['Evacuate vm-1 before maintenance'],
              estimated_downtime_sec: 120,
              vms_at_risk: 1,
            },
            {
              action: 'isolate',
              target: 'default',
              severity: 'medium',
              summary: 'Batch: network isolate blocks east-west traffic',
              affected_vms: [],
              affected_applications: [],
              storage_risks: [],
              network_notes: ['East-west blocked'],
              recommendations: [],
              estimated_downtime_sec: 0,
              vms_at_risk: 0,
            },
          ],
        },
      })
    }
    if (url.includes('/ai/twin/impact') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          action: 'shutdown',
          target: 'host-1',
          severity: 'high',
          summary: '1 VM would lose compute if host-1 shuts down',
          affected_vms: ['vm-1'],
          affected_applications: ['web-tier'],
          storage_risks: [],
          network_notes: ['Default bridge segment isolated'],
          recommendations: ['Evacuate vm-1 before maintenance'],
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/topology/)) {
      return route.fulfill({
        json: {
          nodes: [{ id: 'n1', name: 'vm-1', kind: 'vm' }, { id: 'n2', name: 'host-1', kind: 'host' }],
          edges: [{ from: 'n1', to: 'n2', label: 'runs_on' }],
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/ha(\?|$)/)) {
      return route.fulfill({ json: { vm_id: 'v1', enabled: false, restart_policy: 'restart' } })
    }
    if (url.match(/\/vms\/[^/]+\/spec/)) {
      return route.fulfill({
        json: {
          domain: { name: 'vm-1' },
          cloud_init: { user: 'ubuntu', ssh_pubkey: 'ssh-ed25519 AAA test' },
        },
      })
    }
    if (url.match(/\/zeus-firewall\/vms\/[^/]+\/guest-ports/)) {
      return route.fulfill({
        json: {
          vm_id: 'v1',
          vm_name: 'vm-1',
          agent_reachable: true,
          summary: '2 listening port(s)',
          ports: [
            { port: 22, protocol: 'tcp', service_name: 'ssh', bind_address: '0.0.0.0', allowed_from: [], risk: 'Safe', evidence: [] },
            { port: 80, protocol: 'tcp', service_name: 'http', bind_address: '0.0.0.0', allowed_from: [], risk: 'Warning', evidence: [] },
          ],
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/ws-token/) && route.request().method() === 'POST') {
      return route.fulfill({ json: { token: 'mock-ws-token' } })
    }
    if (url.match(/\/vms\/[^/]+\/consolehub\/plan/)) {
      const vmId = url.match(/\/vms\/([^/]+)\/consolehub\/plan/)?.[1]
      const isKubevirt = vmId === 'kv1'
      const isSpice = vmId === 'sp1'
      const sshRule = portForwardRules.find((r) => r.vm_port === 22)
      return route.fulfill({
        json: {
          vm_id: vmId ?? 'v1',
          vm_name: isKubevirt ? 'kv-vm-1' : isSpice ? 'spice-vm-1' : 'vm-1',
          recommended: isKubevirt ? 'novnc' : isSpice ? 'webrtc_spice' : 'novnc',
          native: isKubevirt
            ? { console_type: 'vnc', ws_path: '/ws/v1/platform/vnc/kv1?token=mock-ws-token', serial_ws_path: '/ws/v1/platform/serial/kv1?token=mock-ws-token', available: true }
            : isSpice
              ? { console_type: 'spice', ws_path: '/ws/v1/platform/spice/sp1?token=mock-ws-token', serial_ws_path: null, available: true }
              : { console_type: 'vnc', ws_path: '/ws/v1/platform/vnc/v1?token=mock-ws-token', serial_ws_path: '/ws/v1/platform/serial/v1?token=mock-ws-token', available: true },
          guest_ip: isKubevirt ? null : '192.168.122.10',
          ssh_user: 'ubuntu',
          os_hint: 'linux',
          protocols: isKubevirt
            ? ['serial', 'novnc']
            : isSpice
              ? ['spice', 'webrtc_spice', 'novnc', 'serial', 'native_ssh']
              : ['novnc', 'native_ssh', 'novnc', 'serial', 'native_ssh'],
          webrtc_spice_available: isSpice,
          guest_access: isKubevirt
            ? null
            : {
                auth_mode: 'ssh_key',
                serial_password_login: false,
                guest_ip_private: true,
                ssh_nat_host_port: sshRule?.host_port ?? null,
              },
          hypervisor_address: 'lab.test',
          ssh_connect_host: isKubevirt ? null : sshRule ? 'lab.test' : null,
          ssh_connect_port: isKubevirt ? null : sshRule?.host_port ?? null,
          session_recording_enabled: true,
          permissions: {
            role: 'admin',
            read_only: false,
            can_power: true,
            can_snapshot: true,
            can_send_keys: true,
          },
        },
      })
    }
    if (url.includes('/consolehub/spectator/validate')) {
      const params = new URL(url).searchParams
      const token = params.get('token')
      const sessionId = params.get('session_id')
      const valid = token === 'mock-spectator' && sessionId === '00000000-0000-4000-8000-000000000002'
        || token === 'mock-collab-spectator' && sessionId === '00000000-0000-4000-8000-0000000000aa'
      return route.fulfill({
        json: valid
          ? { valid: true, vm_id: 'v1', actor: 'admin', protocol: 'novnc', read_only: true }
          : { valid: false, vm_id: '', actor: '', protocol: '', read_only: true },
      })
    }
    if (url.includes('/consolehub/break-glass') && route.request().method() === 'POST') {
      const breakVmId = url.match(/\/vms\/([^/]+)\/consolehub\/break-glass/)?.[1] ?? 'v1'
      return route.fulfill({
        json: {
          session_id: '00000000-0000-4000-8000-000000000099',
          vm_id: breakVmId,
          protocol: 'novnc',
          backend: 'native',
          embed_path: '/platform/vms/v1/consolehub',
          audit_id: 'audit-1',
          expires_at: new Date(Date.now() + 3_600_000).toISOString(),
          recording_enabled: true,
          spectator_token: 'mock-spectator',
        },
      })
    }
    if (url.includes('/consolehub/collaborate') && route.request().method() === 'POST') {
      const collabVmId = url.match(/\/vms\/([^/]+)\/consolehub\/collaborate/)?.[1] ?? 'v1'
      const sessionId = '00000000-0000-4000-8000-0000000000aa'
      const token = 'mock-collab-spectator'
      return route.fulfill({
        json: {
          session_id: sessionId,
          vm_id: collabVmId,
          protocol: 'novnc',
          backend: 'native',
          embed_path: `/platform/vms/${collabVmId}/consolehub?mode=cinema&session=${sessionId}&spectator=${token}`,
          audit_id: 'audit-collab',
          expires_at: new Date(Date.now() + 3_600_000).toISOString(),
          recording_enabled: true,
          spectator_token: token,
        },
      })
    }
    if (url.match(/\/consolehub\/sessions\/[^/]+\/replay/) && route.request().method() === 'GET') {
      return route.fulfill({
        status: 200,
        contentType: 'video/webm',
        body: Buffer.from([0x1a, 0x45, 0xdf, 0xa3]),
      })
    }
    if (url.match(/\/consolehub\/sessions\/[^/]+\/replay/) && route.request().method() === 'PUT') {
      return route.fulfill({ json: { uploaded: true, bytes: 4 } })
    }
    if (url.includes('/consolehub/sessions') && route.request().method() === 'GET') {
      return route.fulfill({ json: mockConsoleHubSessions })
    }
    if (url.includes('/consolehub/sessions') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          session_id: '00000000-0000-4000-8000-000000000099',
          vm_id: 'v1',
          protocol: 'native_ssh',
          backend: 'native',
          embed_path: '/platform/vms/00000000-0000-4000-8000-000000000099/consolehub?native=1',
          emergency_url: null,
          audit_id: '00000000-0000-4000-8000-000000000098',
          expires_at: new Date(Date.now() + 600_000).toISOString(),
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/console(?!hub)/)) {
      return route.fulfill({
        json: { vm_id: 'v1', vm_name: 'vm-1', console_type: 'vnc', ws_path: '/ws/v1/platform/vnc/v1?token=mock-ws-token' },
      })
    }
    if (url.match(/\/vms\/guest-health-fail\/guest\/health/)) {
      return route.fulfill({ status: 500, json: { error: 'guest health unavailable' } })
    }
    if (url.match(/\/vms\/[^/]+\/guest\/observability/)) {
      return route.fulfill({
        json: {
          hostname: 'vm-1',
          os_pretty_name: 'Ubuntu 24.04 LTS',
          cloud_init_status: 'done',
          filesystems: [{ mountpoint: '/', fs_type: 'ext4', used_bytes: 12e9, total_bytes: 40e9 }],
          users: [{ username: 'ubuntu', login_time: new Date().toISOString() }],
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/guest\/fs-freeze-status/)) {
      return route.fulfill({
        json: {
          action: 'fs_freeze_status',
          ok: true,
          message: 'Filesystems not frozen',
          fs_freeze: { frozen: false, detail: 'No active quiesce' },
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/guest\/health/)) {
      return route.fulfill({
        json: {
          vm_id: 'v1',
          vm_name: 'vm-1',
          agent_reachable: true,
          healthy: true,
          os_pretty_name: 'Ubuntu 24.04 LTS',
          guest_ip: '192.168.122.50',
          guest_hostname: 'vm-1',
          issues: [],
          summary: 'Guest agent running · Ubuntu 24.04 LTS',
          install_state: 'running',
          channel_attached: true,
          channel_connected: true,
          agent_ping: true,
          agent_version: '6.2.0',
          checks: [{ id: 'agent_ping', label: 'QEMU guest agent ping', passed: true, detail: 'OK' }],
          guest_observability: {
            os_pretty_name: 'Ubuntu 24.04 LTS',
            os_kernel: '6.8.0',
            ip_addresses: [{ name: 'eth0', address: '192.168.122.50', source: 'agent', ip_type: 'ipv4' }],
            users: [{ username: 'ubuntu', login_time: new Date().toISOString() }],
            time: { guest_time_rfc3339: new Date().toISOString(), host_time_rfc3339: new Date().toISOString(), delta_ms: 1200 },
          },
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/guest\/ai-insights/) && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          vm_id: 'v1',
          vm_name: 'vm-1',
          summary: 'Ubuntu 24.04 with QGA active — time drift minor.',
          insights: [{ title: 'Guest agent active', severity: 'info', detail: 'guest-ping OK' }],
          recommendations: [{ label: 'Sync guest time', action: 'guest.sync_time', risk: 'low', rationale: 'Minor drift' }],
          llm_powered: false,
          snapshot: { install_state: 'running', agent_ping: true },
        },
      })
    }
    if (url.includes('/ai/fleet/guest-query') && route.request().method() === 'POST') {
      const raw = route.request().postData() ?? ''
      let body: { query?: string } = {}
      try {
        body = JSON.parse(raw) as { query?: string }
      } catch { /* empty */ }
      if (body.query?.includes('empty-test') || raw.includes('empty-test')) {
        return route.fulfill({
          json: {
            query: body.query,
            summary: 'No VMs matched for empty-test query.',
            matched_count: 0,
            scanned_count: 3,
            llm_powered: false,
            vms: [],
          },
        })
      }
      return route.fulfill({
        json: {
          query: 'guest agent',
          summary: '1 VM matched with active guest agent.',
          matched_count: 1,
          scanned_count: 1,
          llm_powered: false,
          vms: [{ vm_id: 'v1', vm_name: 'vm-1', os_pretty_name: 'Ubuntu 24.04', guest_ip: '192.168.122.50', install_state: 'running', user_count: 1, flags: [] }],
        },
      })
    }
    if (url.includes('/ai/migration/readiness-report') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          executive_summary: 'Fleet migration readiness: average 85%. Enable QGA on all guests before cutover.',
          vm_count: 1,
          rows: [{ vm_id: 'v1', vm_name: 'vm-1', readiness_percent: 85, install_state: 'running', os_pretty_name: 'Ubuntu 24.04', guest_ip: '192.168.122.50', qga_gaps: [], remediation: [] }],
          prioritized_remediation: ['Verify virtio drivers post-migration'],
          llm_powered: false,
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/domain-xml/)) {
      return route.fulfill({
        json: {
          xml: `<domain type='kvm'>
  <name>vm-1</name>
  <memory unit='KiB'>2097152</memory>
  <vcpu placement='static'>2</vcpu>
  <os firmware='efi'>
    <loader secure='yes'>/usr/share/OVMF/OVMF_CODE.secboot.fd</loader>
  </os>
  <cpu mode='host-model' check='partial'/>
  <memballoon model='virtio'/>
  <tpm model='tpm-crb'><backend type='emulator' version='2.0'/></tpm>
  <video><model type='virtio'/></video>
  <graphics type='vnc' port='-1' listen='127.0.0.1'/>
  <graphics type='spice' port='-1' listen='127.0.0.1'/>
  <interface type='network'><mac address='52:54:00:12:34:56'/><source network='default'/><model type='virtio'/></interface>
  <disk type='file' device='disk'><target dev='vda' bus='virtio'/></disk>
  <hostdev mode='subsystem' type='pci' managed='yes'>
    <source>
      <address domain='0x0000' bus='0x03' slot='0x00' function='0x0'/>
    </source>
  </hostdev>
</domain>`,
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/health-check/) && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          vm_id: 'v1',
          vm_name: 'vm-1',
          score: 'ok',
          healthy: true,
          checks_passed: 3,
          checks_total: 3,
          issues: [],
          guest_tools_status: 'installed',
          guest_ip: '192.168.122.50',
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/delete/) && route.request().method() === 'POST') {
      return route.fulfill({
        json: { task_id: 'task-delete-mock', status: 'pending', operation: 'vm.delete' },
      })
    }
    if (url.match(/\/vms\/[^/]+\/(shutdown|pause|resume|start|stop|reboot)$/) && route.request().method() === 'POST') {
      return route.fulfill({ json: { task_id: 'task-power-mock' } })
    }
    if (url.match(/\/vms\/[^/]+\/snapshots/)) {
      return route.fulfill({ json: [] })
    }
    if (url.match(/\/vms\/[^/]+\/backups/)) {
      return route.fulfill({ json: [] })
    }
    if (url.match(/\/vms\/[^/]+\/schedules/) && route.request().method() !== 'DELETE') {
      if (route.request().method() === 'POST') {
        return route.fulfill({
          json: { id: 'sched-mock-1', vm_id: 'v1', action: 'snapshot', interval_minutes: 1440, retention: 5, label: '', enabled: true, next_run_at: '2026-06-27 00:00:00', last_run_at: null, created_at: '2026-06-26 00:00:00' },
        })
      }
      return route.fulfill({ json: [] })
    }
    if (url.match(/\/vms\/[^/]+\/schedules\/[^/]+/) && route.request().method() === 'DELETE') {
      return route.fulfill({ json: { ok: true } })
    }
    if (url.match(/\/vms\/[^/]+\/disks\/attach/) && route.request().method() === 'POST') {
      return route.fulfill({ json: { task_id: 'task-attach-disk-mock', status: 'pending' } })
    }
    if (url.match(/\/vms\/[^/]+\/disks\/resize\//) && route.request().method() === 'POST') {
      return route.fulfill({ json: { task_id: 'task-resize-disk-mock', status: 'pending' } })
    }
    if (url.match(/\/vms\/[^/]+\/nics\/attach/) && route.request().method() === 'POST') {
      return route.fulfill({ json: { task_id: 'task-attach-nic-mock', status: 'pending' } })
    }
    if (url.match(/\/vms\/[^/]+\/disks/)) {
      return route.fulfill({ json: [] })
    }
    if (url.match(/\/vms\/[^/]+\/metrics/)) {
      const vmId = url.match(/\/vms\/([^/]+)\/metrics/)?.[1] ?? 'v1'
      return route.fulfill({
        json: {
          vm_id: vmId,
          cpu_percent: 12,
          memory_used_mib: 768,
          memory_percent: 40,
          updated_at: new Date().toISOString(),
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/migrations/)) {
      return route.fulfill({
        json: [{
          id: 'mig-1', vm_id: 'v1', source_host_id: 'h1', dest_host_id: 'h2',
          source_host: 'host-1', dest_host: 'host-2', live: true, status: 'completed',
          progress: 100, message: null, created_at: new Date(Date.now() - 3_600_000).toISOString(),
        }],
      })
    }
    if (url.match(/\/vms\/[^/]+\/timeline/)) {
      return route.fulfill({ json: [] })
    }
    if (url.match(/\/vms\/[^/]+\/port-forward-templates/)) {
      if (route.request().method() === 'POST') {
        const body = route.request().postDataJSON() as { id: string; name: string; vm_port: number; host_port: number; access: string }
        return route.fulfill({ json: [body] })
      }
      return route.fulfill({ json: [] })
    }
    if (url.match(/\/vms\/[^/]+\/port-forwards\/delete/) && route.request().method() === 'POST') {
      const body = route.request().postDataJSON() as { host_port?: number; vm_port?: number }
      const idx = portForwardRules.findIndex(
        (r) => r.host_port === body.host_port && r.vm_port === body.vm_port,
      )
      if (idx >= 0) portForwardRules.splice(idx, 1)
      return route.fulfill({ json: { ok: true } })
    }
    if (url.match(/\/vms\/[^/]+\/port-forwards/) && route.request().method() === 'POST') {
      const body = route.request().postDataJSON() as {
        host_port: number
        vm_port: number
        protocol?: string
        vm_ip?: string
        description?: string
      }
      portForwardRules.push({
        id: `pf-${portForwardRules.length + 1}`,
        protocol: body.protocol ?? 'tcp',
        host_port: body.host_port,
        vm_ip: body.vm_ip ?? '192.168.122.10',
        vm_port: body.vm_port,
        description: body.description ?? vmFixture.name,
      })
      return route.fulfill({ json: { ok: true } })
    }
    if (url.match(/\/vms\/[^/]+\/port-forwards/)) {
      return route.fulfill({ json: portForwardRules })
    }
    if (url.match(/\/vms\/[^/]+\/doctor/)) {
      return route.fulfill({
        json: {
          vm_id: vmFixture.id,
          vm_name: vmFixture.name,
          score: 'warn',
          score_numeric: 62,
          score_label: 'Degraded',
          healthy: false,
          checks_passed: 2,
          checks_total: 4,
          issues: [{ id: 'power', severity: 'warn', message: 'VM is not running' }],
          guest_tools_status: 'unknown',
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/migrate\/precheck/) && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          ok: true,
          checks: [{ name: 'storage', passed: true, message: 'Shared storage reachable' }],
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/migrate/) && route.request().method() === 'POST') {
      return route.fulfill({ json: { task_id: 'task-migrate-abc123def456' } })
    }
    if (url.includes('/vms/guest-ips/batch') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          items: {
            [vmFixture.id]: { guest_ip: '192.168.122.50', nic_ips: ['192.168.122.50', '10.0.0.5'] },
          },
        },
      })
    }
    if (url.includes('/hardware-summary')) {
      return route.fulfill({
        json: {
          vm_name: 'vm-1',
          state: 'running',
          cpu: { label: 'CPU', value: '2 vCPU · host-passthrough · Skylake-Client', badges: [] },
          memory: { label: 'Memory', value: '2 GiB · balloon enabled', badges: ['live'] },
          firmware: { label: 'Firmware', value: 'UEFI · Secure Boot enabled', badges: ['restart_required'] },
          tpm: { label: 'TPM', value: 'TPM 2.0 emulator · persistent', badges: ['restart_required'] },
          display: { label: 'Display', value: 'SPICE / VNC', badges: [] },
          video: { label: 'Video', value: 'qxl', badges: [] },
          disk_bus: { label: 'Disk bus', value: 'virtio', badges: [] },
          nic: { label: 'NIC', value: 'default · virtio', badges: [] },
          guest_agent: { label: 'Guest agent', value: 'running', badges: [] },
          host_devices: { label: 'Host devices', value: 'none', badges: [] },
          migration: { label: 'Migration', value: 'safe', badges: [] },
          windows_readiness: null,
          balloon_enabled: true,
          secure_boot: true,
          has_vfio_hostdev: false,
          needs_shutdown: false,
        },
      })
    }
    if (url.includes('/hardware-compat')) {
      return route.fulfill({
        json: {
          ok: true,
          issues: [],
          cpu_modes_supported: ['host-passthrough', 'custom'],
          tpm_supported: true,
          uefi_supported: true,
        },
      })
    }
    if (url.includes('/domain-caps')) {
      return route.fulfill({
        json: {
          arch: 'x86_64',
          virttype: 'kvm',
          cpu_modes_supported: ['host-passthrough', 'custom', 'maximum'],
          machine_types: ['pc-q35-8.2', 'pc-i440fx-8.2'],
          tpm_supported: true,
          uefi_supported: true,
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/pending-config/)) {
      return route.fulfill({ json: { needs_shutdown: false, state: 'running', persistent: true, pending_changes: [] } })
    }
    if (url.match(/\/vms\/[^/]+\/libvirt-details/)) {
      return route.fulfill({
        json: {
          name: 'vm-1',
          uuid: '00000000-0000-4000-8000-000000000001',
          state: 'running',
          vcpus: 2,
          memory_mb: 2048,
          os_type: 'hvm',
          arch: 'x86_64',
          autostart: true,
          persistent: true,
          interfaces: [{ mac_address: '52:54:00:12:34:56', ip: '192.168.122.50', source: 'default', model: 'virtio' }],
          disks: [
            { target: 'vda', device: 'disk', source: '/var/lib/libvirt/images/vm-1.qcow2', bus: 'virtio' },
            { target: 'sda', device: 'cdrom', source: '/var/lib/libvirt/images/debian-12.iso', bus: 'sata' },
          ],
          filesystems: [{ source: '/var/share', mount_tag: 'share' }],
        },
      })
    }
    if (url.match(/\/vms\/[^/]+\/libvirt(\?|$)/)) {
      const action = new URL(url).searchParams.get('action') ?? ''
      if (action === 'cpu.memory.topology') {
        return route.fulfill({
          json: {
            sockets: 1,
            cores: 2,
            threads: 1,
            vcpus: 2,
            current_memory_kib: 2_097_152,
            max_memory_kib: 4_194_304,
            state: 'running',
            has_vfio_hostdev: false,
          },
        })
      }
      if (action === 'snapshot.precheck') {
        return route.fulfill({ json: { allowed: true, warnings: [], has_vfio: false } })
      }
      if (action === 'snapshot.action.precheck') {
        return route.fulfill({
          json: {
            allowed: true,
            warnings: [],
            action: new URL(url).searchParams.get('snapshot_action') ?? 'revert',
          },
        })
      }
      if (route.request().method() === 'POST') {
        return route.fulfill({ json: { ok: true, task_id: 'task-libvirt-mock' } })
      }
    }
    if (url.match(/\/vms\/[^/]+(\?|$)/) || url.match(/\/vms\/[^/]+$/)) {
      const vmId = url.match(/\/vms\/([^/?]+)/)?.[1]
      if (vmId === 'kv1') return route.fulfill({ json: kubevirtVmFixture })
      if (vmId === 'sp1') return route.fulfill({ json: spiceVmFixture })
      return route.fulfill({ json: vmFixture })
    }
    if (url.includes('/vms')) {
      const missingVm = { ...vmFixture, id: 'v-missing', name: 'ghost-vm', observed_state: 'missing', last_error: 'domain missing from libvirt' }
      if (url.includes('folder=missing')) {
        return route.fulfill({ json: [missingVm] })
      }
      return route.fulfill({ json: [vmFixture, kubevirtVmFixture, spiceVmFixture, missingVm] })
    }
    if (url.includes('/zeus-firewall/multisite/dr-templates')) {
      return route.fulfill({
        json: {
          summary: '2 DR profile pairs',
          profiles: [{ primary_profile: 'ProductionServer', dr_profile: 'DRServer' }],
        },
      })
    }
    if (url.includes('/zeus-firewall/policies')) {
      return route.fulfill({ json: [{ id: 'p1', name: 'production-default', profile: 'ProductionServer', enabled: true }] })
    }
    if (url.match(/\/api\/v1\/tasks\/[^/]+$/) && route.request().method() === 'GET') {
      const taskId = url.split('/').pop() ?? ''
      const task = platformTasks.find((t) => t.id === taskId || t.id.startsWith(taskId)) ?? platformTasks[0]
      return route.fulfill({ json: task })
    }
    if (url.match(/\/api\/v1\/tasks(\?|$)/)) {
      return route.fulfill({ json: platformTasks })
    }
    if (url.includes('/ai/autopilot/run') && route.request().method() === 'POST') {
      return route.fulfill({
        json: {
          executed_count: 1,
          skipped_count: 0,
          results: [{ message: 'Restarted stale guest agent on vm-1' }],
        },
      })
    }
    if (url.includes('/tasks')) {
      return route.fulfill({ json: platformTasks })
    }
    if (url.includes('/notifications')) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/backup-targets')) {
      return route.fulfill({ json: [{ id: 't1', name: 'nfs-primary', kind: 'nfs', config_json: {} }] })
    }
    if (url.includes('/developer/overview')) {
      return route.fulfill({
        json: {
          summary: 'Developer SDK',
          openapi_url: '/api/v1/openapi.json',
          sdk_typescript: { version: '1.0', path: 'sdk/typescript', install: 'npm install @zyvor/machina-sdk', resources: ['hosts', 'vms'] },
          terraform: { provider_source: 'registry', examples_path: 'examples', resources: [{ name: 'machina_vm' }] },
        },
      })
    }
    if (url.includes('/openapi.json')) {
      const paths: Record<string, Record<string, { summary: string; tags?: string[] }>> = {}
      for (let i = 0; i < 60; i += 1) {
        paths[`/api/v1/hosts/h${i}`] = { get: { summary: `Get host ${i}`, tags: ['hosts'] } }
      }
      paths['/api/v1/hosts'] = { get: { summary: 'List hosts', tags: ['hosts'] } }
      paths['/api/v1/vms'] = { get: { summary: 'List VMs', tags: ['vms'] } }
      paths['/api/v1/health'] = { get: { summary: 'Health check', tags: ['health'] } }
      return route.fulfill({ json: { openapi: '3.0.3', paths } })
    }
    if (url.includes('/users/prune-invalid') && route.request().method() === 'POST') {
      return route.fulfill({ json: { deleted: 0 } })
    }
    if (url.includes('/users/me')) {
      return route.fulfill({ json: { id: 'u1', username: 'admin', role: 'admin' } })
    }
    if (url.match(/\/api\/v1\/users(\?|$)/) && route.request().method() === 'GET') {
      return route.fulfill({ json: [{ id: 'u1', username: 'admin', role: 'admin' }] })
    }
    if (url.match(/\/api\/v1\/api-keys(\?|$)/)) {
      return route.fulfill({ json: [{ id: 'k1', name: 'automation', role: 'operator', created_at: new Date().toISOString() }] })
    }
    if (url.match(/\/api\/v1\/alert-rules\/[^/]+$/)) {
      const id = url.split('/').pop()!.split('?')[0]
      if (route.request().method() === 'PATCH') {
        const idx = alertRules.findIndex((r) => r.id === id)
        if (idx < 0) return route.fulfill({ status: 404, json: { error: 'alert rule not found' } })
        const body = (route.request().postDataJSON() ?? {}) as { enabled: boolean }
        alertRules[idx] = { ...alertRules[idx], enabled: body.enabled }
        return route.fulfill({ json: alertRules[idx] })
      }
      if (route.request().method() === 'DELETE') {
        alertRules = alertRules.filter((r) => r.id !== id)
        return route.fulfill({ json: { deleted: true } })
      }
    }
    if (url.match(/\/api\/v1\/alert-rules(\?|$)/)) {
      if (route.request().method() === 'POST') {
        const body = (route.request().postDataJSON() ?? {}) as Partial<(typeof alertRules)[number]>
        const row = {
          id: `ar${alertRules.length + 1}`, name: body.name ?? 'rule', metric: body.metric ?? 'cpu_percent',
          comparator: body.comparator ?? 'gt', threshold: body.threshold ?? 80, severity: body.severity ?? 'warning',
          scope_project: body.scope_project ?? '', scope_tag: body.scope_tag ?? '',
          cooldown_minutes: body.cooldown_minutes ?? 30, enabled: body.enabled ?? true,
          last_fired_at: null, created_at: new Date().toISOString(),
        }
        alertRules = [row, ...alertRules]
        return route.fulfill({ json: row })
      }
      return route.fulfill({ json: alertRules })
    }
    if (url.match(/\/api\/v1\/scheduled-jobs\/[^/]+$/)) {
      const id = url.split('/').pop()!.split('?')[0]
      if (route.request().method() === 'PATCH') {
        const idx = scheduledJobs.findIndex((j) => j.id === id)
        if (idx < 0) return route.fulfill({ status: 404, json: { error: 'scheduled job not found' } })
        const body = (route.request().postDataJSON() ?? {}) as { enabled: boolean }
        scheduledJobs[idx] = { ...scheduledJobs[idx], enabled: body.enabled }
        return route.fulfill({ json: scheduledJobs[idx] })
      }
      if (route.request().method() === 'DELETE') {
        scheduledJobs = scheduledJobs.filter((j) => j.id !== id)
        return route.fulfill({ json: { deleted: true } })
      }
    }
    if (url.match(/\/api\/v1\/scheduled-jobs(\?|$)/)) {
      if (route.request().method() === 'POST') {
        const body = (route.request().postDataJSON() ?? {}) as Partial<(typeof scheduledJobs)[number]>
        const row = {
          id: `sj${scheduledJobs.length + 1}`, name: body.name ?? 'job', operation: body.operation ?? 'host.inventory',
          payload: body.payload ?? '{}', target_host_id: body.target_host_id ?? null,
          interval_minutes: body.interval_minutes ?? 60, enabled: body.enabled ?? true,
          last_run_at: null, created_at: new Date().toISOString(),
        }
        scheduledJobs = [row, ...scheduledJobs]
        return route.fulfill({ json: row })
      }
      return route.fulfill({ json: scheduledJobs })
    }
    if (url.includes('/webhooks/deliveries')) {
      return route.fulfill({ json: [] })
    }
    if (url.match(/\/api\/v1\/webhooks(\?|$)/)) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/fleet/users')) {
      return route.fulfill({
        json: {
          summary: '1 user · 1 admin · 0 workspaces',
          user_count: 1,
          admin_count: 1,
          workspace_count: 0,
          workspaces_enforced: 0,
          workspaces: [],
        },
      })
    }
    if (url.includes('/fleet/spaces')) {
      return route.fulfill({
        json: {
          summary: '1 space · 2 VMs',
          space_count: 1,
          total_vms: 2,
          running_vms: 1,
          spaces: [{
            name: 'default',
            vm_count: 2,
            running_count: 1,
            stopped_count: 1,
            host_count: 1,
            network_isolation: 'shared',
            enforce_quotas: false,
            quota_status: 'none',
          }],
        },
      })
    }
    if (url.includes('/backups/timeline')) {
      return route.fulfill({ json: [] })
    }
    if (url.includes('/ai/security/explain-event')) {
      return route.fulfill({ json: { explanation: 'Routine administrative activity.', risk: 'Low', recommendation: 'Monitor timeline.' } })
    }
    if (url.includes('/ai/security/attack-reconstruct')) {
      return route.fulfill({ json: { attack_chain: ['1. curl downloaded file', '2. payload executed'], summary: '2 steps' } })
    }
    if (url.includes('/ai/security/nl-search')) {
      return route.fulfill({
        json: {
          original_query: 'curl',
          search_query: 'curl',
          hit_count: 1,
          search_backend: 'opensearch',
          llm_powered: false,
          results: { results: [{ summary: 'curl started', host_id: 'h1', severity: 'info' }] },
        },
      })
    }
    if (url.includes('/ai/security/hunt-summary')) {
      return route.fulfill({
        json: {
          summary: '2 correlation finding(s), 4 timeline event(s), 1 high/critical.',
          priority_actions: ['Review high/critical correlations in Security Center'],
          llm_powered: false,
        },
      })
    }
    if (url.includes('/ai/security')) {
      return route.fulfill({
        json: { summary: 'OK', findings: [], remediations: [], status: 'idle' },
      })
    }
    if (url.includes('/ai/')) {
      return route.fulfill({
        json: { summary: 'OK', remediations: [], forecasts: [], highlights: [], status: 'idle', tagline: 'OK' },
      })
    }
    if (url.includes('/zeus-firewall/overview')) {
      return route.fulfill({
        json: {
          summary: '1 target monitored',
          critical_count: 1,
          warning_count: 0,
          profiles: ['ProductionServer'],
          targets: [{
            id: 'h1',
            name: 'host-1',
            hostname: 'host-1',
            kind: 'host',
            risk: 'critical',
            score: 72,
            open_ports: 3,
            enabled: true,
            backend: 'nftables',
            agent_reachable: true,
            blocked_today: 0,
            profile: 'ProductionServer',
          }],
        },
      })
    }
    if (url.includes('/zeus-firewall/')) {
      return route.fulfill({ json: { summary: 'Zeus firewall mock', targets: [], profiles: [] } })
    }
    return route.fulfill({ json: [] })
  })

  // Registered after the catch-all api handler so Playwright matches this route first.
  await page.route('**/fleet/mission**', async (route) => {
    await route.fulfill({ json: fleetMission })
  })
}
