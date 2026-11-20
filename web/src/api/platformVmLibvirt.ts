// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

type VmLibvirtInvokeAction =
  | 'block.commit'
  | 'block.pull'
  | 'block.job.abort'
  | 'disk.tune'
  | 'nic.tune'
  | 'memtune.set'
  | 'scheduler.set'
  | 'vcpu.pin'
  | 'live.vcpus'
  | 'live.memory'
  | 'usb.attach'
  | 'usb.detach'
  | 'pci.attach'
  | 'pci.detach'
  | 'firmware.set'
  | 'tpm.attach'
  | 'tpm.detach'
  | 'vsock.attach'
  | 'vsock.detach'
  | 'virtiofs.add'
  | 'virtiofs.remove'
  | 'watchdog.attach'
  | 'cdrom.insert'
  | 'cdrom.detach'
  | 'cdrom.eject'
  | 'boot.set'
  | 'domain.xml.update'
  | 'cpu.topology.set'

type VmLibvirtAction = VmLibvirtInvokeAction

type HostLibvirtQueryAction =
  | 'browse.isos'
  | 'host.usb'
  | 'host.pci'
  | 'host.node_devices'
  | 'osinfo.list'
  | 'osinfo.detect'
  | 'storage.pools.list'
  | 'storage.volumes.list'
  | 'networks.list'

export type HostLibvirtInvokeAction =
  | 'storage.pool.start'
  | 'storage.pool.stop'
  | 'storage.pool.refresh'
  | 'storage.pool.autostart'
  | 'storage.pool.delete'
  | 'storage.volume.create'
  | 'storage.volume.delete'
  | 'network.start'
  | 'network.stop'
  | 'network.autostart'
  | 'network.delete'

type VmLibvirtQueryAction =
  | 'block.job'
  | 'cputune.get'
  | 'memtune.get'
  | 'boot.get'
  | 'parity.summary'
  | 'cpu.memory.topology'
  | 'snapshot.precheck'
  | 'snapshot.action.precheck'

export type SnapshotPrecheck = {
  ok: boolean
  blocked: boolean
  message: string
  has_vfio_hostdev: boolean
  estimated_bytes: number
  available_bytes?: number | null
}

type SnapshotActionPrecheck = {
  ok: boolean
  blocked: boolean
  message: string
  vm_running: boolean
  external_snapshot: boolean
  has_vfio_hostdev: boolean
}

export type CpuMemoryTopology = {
  vcpus: number
  sockets: number
  cores: number
  threads: number
  current_memory_kib: number
  max_memory_kib: number
  state: string
  has_vfio_hostdev: boolean
}

export const queryVmLibvirt = <T = unknown>(
  vmId: string,
  action: VmLibvirtQueryAction,
  params?: Record<string, string | boolean | undefined>,
) => {
  const sp = new URLSearchParams({ action })
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      if (v !== undefined && v !== '') sp.set(k, String(v))
    }
  }
  return platformFetch<T>(`/api/v1/vms/${vmId}/libvirt?${sp}`)
}

export const precheckVmSnapshot = (
  vmId: string,
  body: { name: string; disk_only?: boolean; quiesce?: boolean; storage_mode?: string },
) =>
  queryVmLibvirt<SnapshotPrecheck>(vmId, 'snapshot.precheck', {
    name: body.name,
    disk_only: body.disk_only,
    quiesce: body.quiesce,
    storage_mode: body.storage_mode,
  })

export const precheckVmSnapshotAction = (
  vmId: string,
  snapshot: string,
  action: 'delete' | 'revert' | 'clone',
) =>
  queryVmLibvirt<SnapshotActionPrecheck>(vmId, 'snapshot.action.precheck', {
    snapshot,
    snapshot_action: action,
  })

export const invokeVmLibvirt = <T = unknown>(
  vmId: string,
  action: VmLibvirtAction,
  payload: Record<string, unknown> = {},
) =>
  platformFetch<T>(`/api/v1/vms/${vmId}/libvirt`, {
    method: 'POST',
    body: JSON.stringify({ action, payload }),
  })

export const putVmDomainXml = (vmId: string, xml: string) =>
  platformFetch<{ status: string }>(`/api/v1/vms/${vmId}/domain-xml`, {
    method: 'PUT',
    body: JSON.stringify({ xml }),
  })

export const queryHostLibvirt = <T = unknown>(
  hostId: string,
  action: HostLibvirtQueryAction,
  params?: Record<string, string>,
) => {
  const sp = new URLSearchParams({ action })
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      if (v) sp.set(k, v)
    }
  }
  return platformFetch<T>(`/api/v1/hosts/${hostId}/libvirt?${sp}`)
}

export const invokeHostLibvirt = <T = unknown>(
  hostId: string,
  action: HostLibvirtInvokeAction,
  payload: Record<string, unknown> = {},
) =>
  platformFetch<T>(`/api/v1/hosts/${hostId}/libvirt`, {
    method: 'POST',
    body: JSON.stringify({ action, payload }),
  })

interface BatchTaskItem {
  vm_id: string
  task_id?: string
  error?: string
}

export const batchVmSnapshot = (body: {
  vm_ids: string[]
  name: string
  description?: string
  disk_only?: boolean
  quiesce?: boolean
}) =>
  platformFetch<{ results: BatchTaskItem[] }>('/api/v1/vms/batch/snapshots', {
    method: 'POST',
    body: JSON.stringify(body),
  })

export const batchVmDelete = (vm_ids: string[], confirmed = true) =>
  platformFetch<{ results: BatchTaskItem[] }>('/api/v1/vms/batch/delete', {
    method: 'POST',
    body: JSON.stringify({ vm_ids, confirmed }),
  })
