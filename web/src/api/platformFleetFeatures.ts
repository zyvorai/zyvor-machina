// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { downloadControllerExport, platformFetch } from './platform'

const json = (method: string, body?: unknown): RequestInit => ({
  method,
  ...(body === undefined ? {} : { body: JSON.stringify(body) }),
})

// ── Failure domains ─────────────────────────────────────────────────────────

export type DomainReport = {
  domain: string
  hosts: number
  hosts_online: number
  vms: number
  ha_vms: number
  memory_total_mib: number
  memory_used_mib: number
  ha_memory_to_recover_mib: number
  headroom_outside_mib: number
  hosts_outside: number
  spare_after_loss_mib: number
  survivable: boolean
}

export type FailureDomainsReport = {
  domains: DomainReport[]
  unassigned_hosts: number
  rack_loss_tolerant: boolean
  findings: string[]
}

export const getFailureDomains = () => platformFetch<FailureDomainsReport>('/api/v1/failure-domains')

// ── Fleet k3s clusters ──────────────────────────────────────────────────────

export type K8sNode = {
  id: string
  cluster_id: string
  host_id: string
  role: string
  node_name: string
  version: string
  status: string
  message: string
}

export type K8sCluster = {
  id: string
  name: string
  version: string
  status: string
  message: string
  api_url: string
  created_by: string
  created_at: string
  updated_at: string
}

export type K8sClusterDetail = K8sCluster & { nodes: K8sNode[] }

export const listK8sClusters = () => platformFetch<K8sCluster[]>('/api/v1/k8s-clusters')
export const getK8sCluster = (id: string) => platformFetch<K8sClusterDetail>(`/api/v1/k8s-clusters/${id}`)
export const createK8sCluster = (body: {
  name: string
  version: string
  /** One server (SQLite datastore). */
  server_host_id?: string
  /** An HA control plane: 3 or 5 servers; the first bootstraps. */
  server_host_ids?: string[]
  agent_host_ids: string[]
}) => platformFetch<K8sClusterDetail>('/api/v1/k8s-clusters', json('POST', body))
export const addK8sNode = (id: string, host_id: string) =>
  platformFetch<K8sClusterDetail>(`/api/v1/k8s-clusters/${id}/nodes`, json('POST', { host_id }))
/** Drains the node first unless `force` is set (pods on a forced removal are lost). */
export const removeK8sNode = (id: string, nodeId: string, force = false) =>
  platformFetch<K8sClusterDetail>(
    `/api/v1/k8s-clusters/${id}/nodes/${nodeId}${force ? '?force=true' : ''}`,
    json('DELETE'),
  )
export const upgradeK8sCluster = (id: string, version: string, skipDrain = false) =>
  platformFetch<K8sClusterDetail>(
    `/api/v1/k8s-clusters/${id}/upgrade`,
    json('POST', { version, skip_drain: skipDrain }),
  )
export const deleteK8sCluster = (id: string) =>
  platformFetch<K8sClusterDetail>(`/api/v1/k8s-clusters/${id}`, json('DELETE'))
/** Downloads the admin kubeconfig (YAML), so it is saved as a file rather than parsed as JSON. */
export const downloadK8sKubeconfig = (id: string, name: string) =>
  downloadControllerExport(`/api/v1/k8s-clusters/${id}/kubeconfig`, `${name}-kubeconfig.yaml`)

// ── Desktop pools ───────────────────────────────────────────────────────────

export type DesktopPool = {
  id: string
  name: string
  template_vm_id: string
  prefix: string
  size: number
  clone_mode: string
  persistent: boolean
  created_by: string
  created_at: string
  updated_at: string
}

export type DesktopMember = {
  id: string
  vm_name: string
  vm_id: string | null
  vm_state: string | null
  assigned_user: string
  assigned_at: string
  state: string
  message: string
}

export type DesktopPoolDetail = DesktopPool & { members: DesktopMember[] }

export const listDesktopPools = () => platformFetch<DesktopPool[]>('/api/v1/desktop-pools')
export const getDesktopPool = (id: string) => platformFetch<DesktopPoolDetail>(`/api/v1/desktop-pools/${id}`)
export const createDesktopPool = (body: {
  name: string
  template_vm_id: string
  size: number
  persistent?: boolean
}) => platformFetch<DesktopPoolDetail>('/api/v1/desktop-pools', json('POST', body))
export const scaleDesktopPool = (id: string, size: number) =>
  platformFetch<DesktopPoolDetail>(`/api/v1/desktop-pools/${id}/scale`, json('POST', { size }))
export const reconcileDesktopPool = (id: string) =>
  platformFetch<DesktopPoolDetail>(`/api/v1/desktop-pools/${id}/reconcile`, json('POST'))
export const checkoutDesktop = (id: string, user?: string) =>
  platformFetch<{ pool: string; user: string; desktop: DesktopMember }>(
    `/api/v1/desktop-pools/${id}/checkout`,
    json('POST', user ? { user } : {}),
  )
export const releaseDesktop = (id: string, user?: string) =>
  platformFetch<DesktopPoolDetail>(`/api/v1/desktop-pools/${id}/release`, json('POST', user ? { user } : {}))
export const deleteDesktopPool = (id: string, deleteVms: boolean) =>
  platformFetch<{ status: string }>(`/api/v1/desktop-pools/${id}${deleteVms ? '?delete_vms=true' : ''}`, json('DELETE'))

// ── Migration waves ─────────────────────────────────────────────────────────

export type WaveItem = {
  id: string
  wave_id: string
  position: number
  name: string
  transiva_job_id: string
  status: string
  phase: string
  progress_percent: number
  message: string
  cutover: string
  cutover_note: string
}

export type MigrationWave = {
  id: string
  name: string
  description: string
  planned_at: string
  status: string
  auto_advance: boolean
  max_parallel: number
  created_by: string
  created_at: string
  updated_at: string
}

export type MigrationWaveDetail = MigrationWave & { items: WaveItem[] }

export const listMigrationWaves = () => platformFetch<MigrationWave[]>('/api/v1/migration-waves')
export const getMigrationWave = (id: string) => platformFetch<MigrationWaveDetail>(`/api/v1/migration-waves/${id}`)
export const createMigrationWave = (body: {
  name: string
  description?: string
  planned_at?: string
  items: { name: string; config: Record<string, unknown> }[]
}) => platformFetch<MigrationWaveDetail>('/api/v1/migration-waves', json('POST', body))
export const startMigrationWave = (id: string, maxParallel: number) =>
  platformFetch<MigrationWaveDetail>(`/api/v1/migration-waves/${id}/start`, json('POST', { max_parallel: maxParallel }))
export const pauseMigrationWave = (id: string) =>
  platformFetch<MigrationWaveDetail>(`/api/v1/migration-waves/${id}/pause`, json('POST'))
export const advanceMigrationWave = (id: string) =>
  platformFetch<MigrationWaveDetail>(`/api/v1/migration-waves/${id}/advance`, json('POST', {}))
export const cancelMigrationWave = (id: string) =>
  platformFetch<MigrationWaveDetail>(`/api/v1/migration-waves/${id}/cancel`, json('POST'))
export const setWaveCutover = (id: string, itemId: string, state: string, note: string) =>
  platformFetch<MigrationWaveDetail>(
    `/api/v1/migration-waves/${id}/items/${itemId}/cutover`,
    json('POST', { state, note }),
  )
export const deleteMigrationWave = (id: string) =>
  platformFetch<{ status: string }>(`/api/v1/migration-waves/${id}`, json('DELETE'))

// ── Backup replication ──────────────────────────────────────────────────────

export type BackupReplica = {
  id: string
  backup_id: string
  target_id: string
  remote_uri: string
  bytes: number
  files: number
  is_dir: boolean
  /** True when the copy has a SHA-256 manifest; false means size-verified only. */
  checksummed: boolean
  manifest_sha256: string
  status: string
  message: string
  verified_at: string
  fetch_status: string
  fetch_path: string
  fetch_message: string
  created_at: string
  updated_at: string
}

export const listBackupReplicas = (params?: { target_id?: string; status?: string }) => {
  const q = new URLSearchParams()
  if (params?.target_id) q.set('target_id', params.target_id)
  if (params?.status) q.set('status', params.status)
  const qs = q.toString()
  return platformFetch<BackupReplica[]>(`/api/v1/backup-replicas${qs ? `?${qs}` : ''}`)
}
export const runBackupReplication = (target_id: string, limit = 10) =>
  platformFetch<{ started: string[]; skipped: string[] }>(
    '/api/v1/backup-replication/run',
    json('POST', { target_id, limit }),
  )
export const fetchBackupReplica = (replicaId: string, host_id: string) =>
  platformFetch<BackupReplica>(`/api/v1/backup-replicas/${replicaId}/fetch`, json('POST', { host_id }))
