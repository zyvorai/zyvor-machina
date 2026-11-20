// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native security groups + rules (controller::api::networking) — Phase 4 of the
// external-cloud-client replacement. Goes through the platform controller proxy.
//
// Groups are advisory (`mode: audit`) until switched to `enforce`; `enforcement` carries what the
// hosts' VM edge actually reports.

import { platformFetch } from './platform'

export interface NativeSecurityGroup {
  id: string
  project_id: string | null
  name: string
  description: string
  enforced: boolean
  mode: 'audit' | 'enforce'
  enforcement?: SecurityGroupEnforcement
}

export type SecurityGroupEnforcementState = 'advisory' | 'pending' | 'enforced' | 'auditing' | 'failed'

export interface SecurityGroupEnforcement {
  state: SecurityGroupEnforcementState
  reason: string
  vms: string[]
}

export interface NativeSecurityGroupRule {
  id: string
  security_group_id: string
  direction: string
  protocol: string | null
  port_min: number | null
  port_max: number | null
  remote_cidr: string | null
  remote_sg_id?: string | null
  description?: string
}

export function listSecurityGroups(): Promise<NativeSecurityGroup[]> {
  return platformFetch<NativeSecurityGroup[]>('/api/v1/security-groups')
}

export function getSecurityGroup(id: string): Promise<NativeSecurityGroup> {
  return platformFetch<NativeSecurityGroup>(`/api/v1/security-groups/${encodeURIComponent(id)}`)
}

export function createSecurityGroup(body: { name: string; description?: string }): Promise<NativeSecurityGroup> {
  return platformFetch<NativeSecurityGroup>('/api/v1/security-groups', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export async function deleteSecurityGroup(id: string): Promise<void> {
  await platformFetch<{ deleted: boolean }>(`/api/v1/security-groups/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  })
}

export function listSecurityGroupRules(groupId: string): Promise<NativeSecurityGroupRule[]> {
  return platformFetch<NativeSecurityGroupRule[]>(`/api/v1/security-groups/${encodeURIComponent(groupId)}/rules`)
}

export function createSecurityGroupRule(
  groupId: string,
  body: {
    direction?: string
    protocol?: string
    port_min?: number
    port_max?: number
    remote_cidr?: string
  },
): Promise<NativeSecurityGroupRule> {
  return platformFetch<NativeSecurityGroupRule>(`/api/v1/security-groups/${encodeURIComponent(groupId)}/rules`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export async function deleteSecurityGroupRule(id: string): Promise<void> {
  await platformFetch<{ deleted: boolean }>(`/api/v1/security-group-rules/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  })
}

export function setSecurityGroupMode(id: string, mode: 'audit' | 'enforce'): Promise<NativeSecurityGroup> {
  return platformFetch<NativeSecurityGroup>(`/api/v1/security-groups/${encodeURIComponent(id)}/mode`, {
    method: 'PUT',
    body: JSON.stringify({ mode }),
  })
}

export function listInstanceSecurityGroups(vmId: string): Promise<NativeSecurityGroup[]> {
  return platformFetch<NativeSecurityGroup[]>(`/api/v1/vms/${encodeURIComponent(vmId)}/security-groups`)
}

export async function attachInstanceSecurityGroup(vmId: string, sgId: string): Promise<void> {
  await platformFetch(`/api/v1/vms/${encodeURIComponent(vmId)}/security-groups/${encodeURIComponent(sgId)}`, {
    method: 'PUT',
  })
}

export async function detachInstanceSecurityGroup(vmId: string, sgId: string): Promise<void> {
  await platformFetch(`/api/v1/vms/${encodeURIComponent(vmId)}/security-groups/${encodeURIComponent(sgId)}`, {
    method: 'DELETE',
  })
}

export interface EnforcePreview {
  instances: { vm: string; ingress_rules: number; egress_rules: number; warnings: string[] }[]
  warning_count: number
}

/** Dry run of switching a group to enforce: what each attached instance would end up with. */
export function previewSecurityGroupEnforcement(id: string): Promise<EnforcePreview> {
  return platformFetch<EnforcePreview>(`/api/v1/security-groups/${encodeURIComponent(id)}/enforce-preview`)
}
