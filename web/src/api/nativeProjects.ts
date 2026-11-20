// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native project registry (controller::api::projects) — Phase 4 of the
// external-cloud-client replacement, the Keystone-project equivalent. Goes through the
// platform controller proxy. There is no native equivalent of Keystone *users*:
// identity/login is Machina's own PAM/OIDC/LDAP/SAML auth, not a per-project user
// catalog — project membership references an existing Machina user by id/role.

import { platformFetch } from './platform'

export interface NativeProject {
  id: string
  name: string
  description: string
  enabled: boolean
}

export interface NativeProjectMember {
  user_id: string
  username: string
  role: string
}

export function listProjectRegistry(): Promise<NativeProject[]> {
  return platformFetch<NativeProject[]>('/api/v1/project-registry')
}

export function createProject(body: { name: string; description?: string }): Promise<NativeProject> {
  return platformFetch<NativeProject>('/api/v1/project-registry', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function listProjectMembers(id: string): Promise<NativeProjectMember[]> {
  return platformFetch<NativeProjectMember[]>(`/api/v1/project-registry/${encodeURIComponent(id)}/members`)
}

export function addProjectMember(id: string, body: { user_id: string; role?: string }): Promise<NativeProjectMember> {
  return platformFetch<NativeProjectMember>(`/api/v1/project-registry/${encodeURIComponent(id)}/members`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export async function removeProjectMember(id: string, userId: string): Promise<void> {
  await platformFetch<{ removed: boolean }>(
    `/api/v1/project-registry/${encodeURIComponent(id)}/members/${encodeURIComponent(userId)}`,
    { method: 'DELETE' },
  )
}

export interface NativeUser {
  id: string
  username: string
  role: string
}

/** Admin-only (controller::api::users::list_users). */
export function listUsers(): Promise<NativeUser[]> {
  return platformFetch<NativeUser[]>('/api/v1/users')
}
