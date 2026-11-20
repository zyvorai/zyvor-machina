// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native L4 load balancer (controller::api::load_balancers) — the last piece of the
// external-cloud-client replacement. One listener (protocol + port) on a chosen host fans out
// to weighted VM:port members via a kernel-level iptables rule set — no amphora VM, no
// external cloud. Deliberately L4-only: no listeners/pools/L7 policies/health-monitor
// hierarchy like Octavia — see controller/migrations/026_native_load_balancers.sql.

import { platformFetch } from './platform'

export interface NativeLoadBalancer {
  id: string
  project_id: string | null
  name: string
  protocol: string
  host_id: string
  listener_port: number
  status: string
  status_message: string
  created_at: string
}

export interface NativeLbMember {
  id: string
  load_balancer_id: string
  vm_id: string
  vm_name: string
  vm_ip: string | null
  port: number
  weight: number
  enabled: boolean
}

export function listLoadBalancers(): Promise<NativeLoadBalancer[]> {
  return platformFetch<NativeLoadBalancer[]>('/api/v1/load-balancers')
}

export function getLoadBalancer(id: string): Promise<NativeLoadBalancer> {
  return platformFetch<NativeLoadBalancer>(`/api/v1/load-balancers/${encodeURIComponent(id)}`)
}

export function createLoadBalancer(body: {
  name: string
  host_id: string
  listener_port: number
  protocol?: string
}): Promise<NativeLoadBalancer> {
  return platformFetch<NativeLoadBalancer>('/api/v1/load-balancers', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export async function deleteLoadBalancer(id: string): Promise<void> {
  await platformFetch(`/api/v1/load-balancers/${encodeURIComponent(id)}`, { method: 'DELETE' })
}

export function listLbMembers(lbId: string): Promise<NativeLbMember[]> {
  return platformFetch<NativeLbMember[]>(`/api/v1/load-balancers/${encodeURIComponent(lbId)}/members`)
}

export function addLbMember(
  lbId: string,
  body: { vm_id: string; port: number; weight?: number },
): Promise<NativeLbMember> {
  return platformFetch<NativeLbMember>(`/api/v1/load-balancers/${encodeURIComponent(lbId)}/members`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function patchLbMember(
  lbId: string,
  memberId: string,
  body: { enabled?: boolean; weight?: number },
): Promise<NativeLbMember> {
  return platformFetch<NativeLbMember>(
    `/api/v1/load-balancers/${encodeURIComponent(lbId)}/members/${encodeURIComponent(memberId)}`,
    { method: 'PATCH', body: JSON.stringify(body) },
  )
}

export async function deleteLbMember(lbId: string, memberId: string): Promise<void> {
  await platformFetch(
    `/api/v1/load-balancers/${encodeURIComponent(lbId)}/members/${encodeURIComponent(memberId)}`,
    { method: 'DELETE' },
  )
}
