// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

export interface VmQosPolicy {
  id: string
  name: string
  project_id: string | null
  egress_mbps: number | null
  ingress_mbps: number | null
  pps: number | null
  description: string
}

export function listVmQosPolicies(): Promise<VmQosPolicy[]> {
  return platformFetch('/api/v1/vm-qos/policies')
}

export function createVmQosPolicy(body: {
  name: string
  egress_mbps?: number | null
  ingress_mbps?: number | null
  pps?: number | null
  description?: string
  project_id?: string | null
}): Promise<VmQosPolicy> {
  return platformFetch('/api/v1/vm-qos/policies', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function deleteVmQosPolicy(id: string): Promise<{ deleted: boolean }> {
  return platformFetch(`/api/v1/vm-qos/policies/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  })
}

export function bindVmQosPolicy(
  id: string,
  vmId: string,
): Promise<{ bound: boolean; edge_pushed?: boolean }> {
  return platformFetch(`/api/v1/vm-qos/policies/${encodeURIComponent(id)}/bind`, {
    method: 'POST',
    body: JSON.stringify({ vm_id: vmId }),
  })
}
