// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { apiPut, readJsonObject } from './client'

const API = '/api/v1'

export interface LdapSettingsView {
  enabled: boolean
  url: string
  base_dn: string
  user_filter: string
  bind_dn: string
  bind_password: string
  bind_password_set: boolean
  user_dn_template: string
  username_attribute: string
  use_tls: boolean
  insecure_tls: boolean
  member_attribute: string
  admin_group_substrings: string[]
  operator_group_substrings: string[]
  readonly_group_substrings: string[]
  config_path: string
  preset?: string | null
}

type LdapSettingsPatch = Partial<
  Omit<LdapSettingsView, 'bind_password_set' | 'config_path' | 'preset'>
> & {
  preset?: string
  bind_password?: string
}

interface LdapTestResponse {
  ok: boolean
  message: string
  username?: string | null
  role?: string | null
}

export const getLdapSettings = () => readJsonObject<LdapSettingsView>(`${API}/system/auth/ldap-settings`)

export const putLdapSettings = (body: LdapSettingsPatch) =>
  apiPut<{ status?: string; restart_recommended?: boolean; restart_message?: string; settings: LdapSettingsView }>(
    `${API}/system/auth/ldap-settings`,
    body,
  )

export async function testLdapBind(username: string, password: string): Promise<LdapTestResponse> {
  const res = await fetch(`${API}/system/auth/ldap-test`, {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username, password }),
  })
  const body = (await res.json().catch(() => ({}))) as LdapTestResponse
  if (!res.ok && !body.message) {
    throw new Error(`LDAP test failed (${res.status})`)
  }
  return body
}

/** Preset for zyvorai.local AD domain (45.82.66.172). */
export const ZYVORAI_AD_PRESET: LdapSettingsPatch = {
  preset: 'zyvorai-local',
  enabled: true,
  url: 'ldap://45.82.66.172:389',
  base_dn: 'DC=zyvorai,DC=local',
  user_filter: '(|(sAMAccountName={username})(userPrincipalName={username}))',
  username_attribute: 'sAMAccountName',
  member_attribute: 'memberOf',
  admin_group_substrings: ['Domain Admins', 'Machina-Admins'],
  operator_group_substrings: ['Machina-Operators'],
  readonly_group_substrings: ['Domain Users'],
}
