// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { parseResponseError } from './parseResponseError'

const API = '/api/v1'

export interface OidcSettingsView {
  enabled: boolean
  issuer_url: string
  client_id: string
  client_secret: string
  client_secret_set: boolean
  redirect_url: string
  scopes: string[]
  username_claim: string
  groups_claim: string
  linux_username_claim: string
  admin_groups: string[]
  operator_groups: string[]
  default_role: string
  button_label: string
  require_local_user_for_session_libvirt: boolean
  config_path: string
  login_path: string
  callback_path: string
}

type OidcSettingsPatch = Partial<{
  enabled: boolean
  issuer_url: string
  client_id: string
  client_secret: string
  redirect_url: string
  scopes: string[]
  username_claim: string
  groups_claim: string
  linux_username_claim: string
  admin_groups: string[]
  operator_groups: string[]
  default_role: string
  button_label: string
  require_local_user_for_session_libvirt: boolean
}>

export interface SamlSettingsView {
  enabled: boolean
  configured: boolean
  sp_entity_id: string
  sp_acs_url: string
  idp_entity_id: string
  idp_metadata_url: string
  idp_metadata_xml: string
  idp_metadata_xml_set: boolean
  name_id_format: string
  button_label: string
  admin_groups: string[]
  operator_groups: string[]
  default_role: string
  notes: string
  config_path: string
  metadata_path: string
}

type SamlSettingsPatch = Partial<{
  enabled: boolean
  sp_entity_id: string
  sp_acs_url: string
  idp_entity_id: string
  idp_metadata_url: string
  idp_metadata_xml: string
  name_id_format: string
  button_label: string
  admin_groups: string[]
  operator_groups: string[]
  default_role: string
  notes: string
}>

export async function getOidcSettings(): Promise<OidcSettingsView> {
  const res = await fetch(`${API}/system/auth/oidc-settings`, { credentials: 'same-origin' })
  if (!res.ok) throw await parseResponseError(res)
  return res.json() as Promise<OidcSettingsView>
}

export async function putOidcSettings(
  patch: OidcSettingsPatch,
): Promise<{ settings: OidcSettingsView; restart_recommended?: boolean; restart_message?: string }> {
  const res = await fetch(`${API}/system/auth/oidc-settings`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'same-origin',
    body: JSON.stringify(patch),
  })
  if (!res.ok) throw await parseResponseError(res)
  return res.json() as Promise<{ settings: OidcSettingsView; restart_recommended?: boolean; restart_message?: string }>
}

export async function getSamlSettings(): Promise<SamlSettingsView> {
  const res = await fetch(`${API}/system/auth/saml-settings`, { credentials: 'same-origin' })
  if (!res.ok) throw await parseResponseError(res)
  return res.json() as Promise<SamlSettingsView>
}

export async function putSamlSettings(
  patch: SamlSettingsPatch,
): Promise<{ settings: SamlSettingsView; restart_recommended?: boolean; restart_message?: string }> {
  const res = await fetch(`${API}/system/auth/saml-settings`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'same-origin',
    body: JSON.stringify(patch),
  })
  if (!res.ok) throw await parseResponseError(res)
  return res.json() as Promise<{ settings: SamlSettingsView; restart_recommended?: boolean; restart_message?: string }>
}

export function suggestedOidcRedirectUrl(): string {
  if (typeof window === 'undefined') return '/api/v1/auth/oidc/callback'
  return `${window.location.origin}/api/v1/auth/oidc/callback`
}

export function suggestedSamlUrls() {
  const origin = typeof window !== 'undefined' ? window.location.origin : 'https://hypervisor.example.com:5092'
  return {
    spEntityId: `${origin}/saml/metadata`,
    spAcsUrl: `${origin}/api/v1/auth/saml/acs`,
    metadataUrl: `${origin}/api/v1/auth/saml/metadata`,
  }
}
