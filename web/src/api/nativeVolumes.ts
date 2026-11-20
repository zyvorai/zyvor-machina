// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native standalone storage volumes (controller::api::volumes) — Phase 4 of the
// external-cloud-client replacement, the Cinder equivalent. Goes through the platform
// controller proxy.
//
// Narrower than Cinder: no volume transfers (project-to-project ownership
// handoff), no retype, no clone, no bootable flag, no create-from-image — none
// of those have a native equivalent yet. Create/list/delete/attach/detach/
// extend/snapshot are fully native.

import { platformFetch } from './platform'

export interface NativeVolume {
  id: string
  project_id: string | null
  name: string
  size_gib: number
  volume_class: string
  status: string
  attached_vm_id: string | null
  attached_device: string | null
  atlas_backed: boolean
  /** EC2-style id, e.g. vol-0123456789abcdef0. */
  ec2_id?: string
  delete_on_termination?: boolean
  read_iops?: number | null
  write_iops?: number | null
  read_bps?: number | null
  write_bps?: number | null
}

export interface VolumeIoLimits {
  read_iops?: number
  write_iops?: number
  read_bps?: number
  write_bps?: number
}

export function setVolumeIoLimits(id: string, limits: VolumeIoLimits): Promise<NativeVolume> {
  return platformFetch<NativeVolume>(`/api/v1/volumes/${encodeURIComponent(id)}/iotune`, {
    method: 'PUT',
    body: JSON.stringify(limits),
  })
}

export function setVolumeDeleteOnTermination(id: string, value: boolean): Promise<NativeVolume> {
  return platformFetch<NativeVolume>(`/api/v1/volumes/${encodeURIComponent(id)}/delete-on-termination`, {
    method: 'PUT',
    body: JSON.stringify({ value }),
  })
}

export interface NativeVolumeSnapshot {
  id: string
  volume_id: string
  name: string
  status: string
}

export interface NativeVolumeSnapshotWithVolume extends NativeVolumeSnapshot {
  volume_name: string
}

export function listVolumes(): Promise<NativeVolume[]> {
  return platformFetch<NativeVolume[]>('/api/v1/volumes')
}

export function getVolume(id: string): Promise<NativeVolume> {
  return platformFetch<NativeVolume>(`/api/v1/volumes/${encodeURIComponent(id)}`)
}

export function createVolume(body: { name: string; size_gib: number; volume_class?: string }): Promise<NativeVolume> {
  return platformFetch<NativeVolume>('/api/v1/volumes', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export async function deleteVolume(id: string): Promise<void> {
  await platformFetch(`/api/v1/volumes/${encodeURIComponent(id)}`, { method: 'DELETE' })
}

export function attachVolume(id: string, body: { vm_id: string; target_dev?: string }): Promise<NativeVolume> {
  return platformFetch<NativeVolume>(`/api/v1/volumes/${encodeURIComponent(id)}/attach`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function detachVolume(id: string): Promise<NativeVolume> {
  return platformFetch<NativeVolume>(`/api/v1/volumes/${encodeURIComponent(id)}/detach`, { method: 'POST' })
}

export function extendVolume(id: string, newSizeGib: number): Promise<NativeVolume> {
  return platformFetch<NativeVolume>(`/api/v1/volumes/${encodeURIComponent(id)}/extend`, {
    method: 'POST',
    body: JSON.stringify({ new_size_gib: newSizeGib }),
  })
}

export function listVolumeSnapshots(volumeId: string): Promise<NativeVolumeSnapshot[]> {
  return platformFetch<NativeVolumeSnapshot[]>(`/api/v1/volumes/${encodeURIComponent(volumeId)}/snapshots`)
}

export function listAllVolumeSnapshots(): Promise<NativeVolumeSnapshotWithVolume[]> {
  return platformFetch<NativeVolumeSnapshotWithVolume[]>('/api/v1/volume-snapshots')
}

export function createVolumeSnapshot(volumeId: string, name: string): Promise<NativeVolumeSnapshot> {
  return platformFetch<NativeVolumeSnapshot>(`/api/v1/volumes/${encodeURIComponent(volumeId)}/snapshots`, {
    method: 'POST',
    body: JSON.stringify({ name }),
  })
}

export async function deleteVolumeSnapshot(id: string): Promise<void> {
  await platformFetch(`/api/v1/volume-snapshots/${encodeURIComponent(id)}`, { method: 'DELETE' })
}

/** EC2 CreateVolume from a snapshot: a new volume cloned from `snapshotId`. */
export function createVolumeFromSnapshot(snapshotId: string, name: string): Promise<NativeVolume> {
  return platformFetch<NativeVolume>(`/api/v1/volume-snapshots/${encodeURIComponent(snapshotId)}/create-volume`, {
    method: 'POST',
    body: JSON.stringify({ name }),
  })
}
