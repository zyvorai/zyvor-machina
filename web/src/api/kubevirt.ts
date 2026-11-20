// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { apiPost, readJsonObject } from './client'
import type { KubeVirtBundle, KubeVirtBundleBody, KubeVirtClusterExecResult } from './vm'

const API = '/api/v1'

/** Standalone qcow2 → KubeVirt (hyper2kvm-style); no libvirt domain required. */
export type Qcow2KubeVirtRequest = {
  qcow2_path: string
  /** `linux` | `windows` | `auto` */
  guest_os?: string
  namespace?: string
  k8s_vm_name?: string
  datavolume_name?: string
  storage_gi?: number
  storage_class?: string
  vcpus?: number
  memory_mb?: number
  include_virtio_cdrom?: boolean
}

export function getQcow2KubeVirtBundle(q: Qcow2KubeVirtRequest) {
  const params = new URLSearchParams()
  params.set('qcow2_path', q.qcow2_path)
  if (q.guest_os) params.set('guest_os', q.guest_os)
  if (q.namespace) params.set('namespace', q.namespace)
  if (q.k8s_vm_name) params.set('k8s_vm_name', q.k8s_vm_name)
  if (q.datavolume_name) params.set('datavolume_name', q.datavolume_name)
  if (q.storage_gi != null) params.set('storage_gi', String(q.storage_gi))
  if (q.storage_class) params.set('storage_class', q.storage_class)
  if (q.vcpus != null) params.set('vcpus', String(q.vcpus))
  if (q.memory_mb != null) params.set('memory_mb', String(q.memory_mb))
  if (q.include_virtio_cdrom != null) params.set('include_virtio_cdrom', String(q.include_virtio_cdrom))
  return readJsonObject<KubeVirtBundle>(`${API}/kubevirt/qcow2-bundle?${params}`)
}

export function postQcow2KubeVirtBundle(body: Qcow2KubeVirtRequest) {
  return apiPost<KubeVirtBundle>(`${API}/kubevirt/qcow2-bundle`, body)
}

export function postQcow2KubeVirtApply(body: Qcow2KubeVirtRequest) {
  return apiPost<KubeVirtClusterExecResult>(`${API}/kubevirt/qcow2/apply`, body)
}

export function postQcow2KubeVirtUpload(body: Qcow2KubeVirtRequest) {
  return apiPost<KubeVirtClusterExecResult>(`${API}/kubevirt/qcow2/upload`, body)
}

export function postQcow2KubeVirtStart(body: Qcow2KubeVirtRequest) {
  return apiPost<KubeVirtClusterExecResult>(`${API}/kubevirt/qcow2/start`, body)
}

export type { KubeVirtBundle, KubeVirtBundleBody, KubeVirtClusterExecResult }
