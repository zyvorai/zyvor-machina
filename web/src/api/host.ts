// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonObject, readJsonItemsList } from './client'

const API = '/api/v1'

interface VirtualizationHostStatus {
  cpu_virt_supported: boolean
  kvm_device_present: boolean
  libvirt_system_socket_present: boolean
  libvirt_session_socket_present: boolean
  hint: string
}

export async function getHostVirtualization(): Promise<VirtualizationHostStatus> {
  return readJsonObject<VirtualizationHostStatus>(`${API}/host/virtualization`)
}

export async function getLibvirtSummary(): Promise<{
  dual_connection: boolean
  primary_connected?: boolean
  qemu_system_connected: boolean
  qemu_session_connected: boolean
  libvirt_connected?: boolean
  configured_uri: string
  extra_uris?: string[]
}> {
  return readJsonObject<{
    dual_connection: boolean
    primary_connected?: boolean
    qemu_system_connected: boolean
    qemu_session_connected: boolean
    libvirt_connected?: boolean
    configured_uri: string
    extra_uris?: string[]
  }>(`${API}/libvirt/summary`)
}

export interface HealthProblemItem {
  id: string
  severity: 'critical' | 'warning'
  title: string
  detail: string
  doc_url: string | null
}

export async function getHealthProblems(): Promise<{ items: HealthProblemItem[] }> {
  return readJsonItemsList<HealthProblemItem>(`${API}/health/problems`)
}

/** Libvirt-related systemd units needed for NAT/QEMU autostart after host reboot (Host overview). */
export interface LibvirtBootStatus {
  needs_attention: boolean
  detail: string | null
  systemd_unit: string | null
}

export const getHostLibvirtBoot = () => readJsonObject<LibvirtBootStatus>(`${API}/host/libvirt-boot`)
