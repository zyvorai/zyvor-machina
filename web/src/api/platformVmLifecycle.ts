// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Retire, portable disk export, IaC bundle export.

import { getControllerBase, platformFetch, platformHeaders } from './platform'

export const retirePlatformVm = (id: string, final_backup = false) =>
  platformFetch<{ task_id: string }>(`/api/v1/vms/${id}/retire`, {
    method: 'POST',
    body: JSON.stringify({ final_backup }),
  })

export const exportVmDisk = (id: string) =>
  platformFetch<{ task_id: string }>(`/api/v1/vms/${id}/disk/export`, { method: 'POST', body: '{}' })

export type VmIacExportBundle = {
  vm_id: string
  vm_name: string
  terraform: string
  ansible_role: string
  cloud_init: string
  domain_xml: string
}

export const exportVmIac = (id: string) => platformFetch<VmIacExportBundle>(`/api/v1/vms/${id}/export`)

/** Download server-generated ZIP (terraform.tf, ansible, cloud-init, domain.xml, manifest.json). */
export async function downloadVmIacZip(vmId: string, vmName: string) {
  const url = `${getControllerBase()}/api/v1/vms/${vmId}/export.zip`
  const res = await fetch(url, { credentials: 'same-origin', headers: platformHeaders() })
  if (!res.ok) {
    const body = await res.text().catch(() => '')
    throw new Error(body || `IaC zip export failed (${res.status})`)
  }
  const blob = await res.blob()
  const a = document.createElement('a')
  a.href = URL.createObjectURL(blob)
  a.download = `${vmName}-iac.zip`
  a.click()
  URL.revokeObjectURL(a.href)
}

/** Download Terraform, Ansible, cloud-init, and domain XML as one JSON bundle. */
export function downloadVmIacBundle(bundle: VmIacExportBundle) {
  const blob = new Blob([JSON.stringify(bundle, null, 2)], { type: 'application/json' })
  const a = document.createElement('a')
  a.href = URL.createObjectURL(blob)
  a.download = `${bundle.vm_name}-iac-bundle.json`
  a.click()
  URL.revokeObjectURL(a.href)
}

export const pruneMissingPlatformVms = () =>
  platformFetch<{ deleted: number }>('/api/v1/vms/prune-missing', { method: 'POST', body: '{}' })

export const pruneStaleVmRecord = (id: string) =>
  platformFetch<{ deleted: boolean; name: string }>(`/api/v1/vms/${id}/prune-inventory`, {
    method: 'POST',
    body: '{}',
  })
