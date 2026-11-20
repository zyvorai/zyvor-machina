// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonArray, apiPost, apiPostVoid, apiDelete, apiPut, apiGetText } from './client'

const API = '/api/v1'

export interface NetworkInfo {
  name: string
  uuid: string
  active: boolean
  persistent: boolean
  autostart: boolean
  bridge: string
}

interface CreateNetworkRequest {
  name: string
  subnet: string
  dhcp_start: string
  dhcp_end: string
}

export const listNetworks = () => readJsonArray<NetworkInfo>(`${API}/networks`)
export const createNetwork = (req: CreateNetworkRequest) => apiPost<unknown>(`${API}/networks`, req)
export const deleteNetwork = (name: string) => apiDelete(`${API}/networks/${encodeURIComponent(name)}`)
export const startNetwork = (name: string) => apiPostVoid(`${API}/networks/${encodeURIComponent(name)}/start`)
export const stopNetwork = (name: string) => apiPostVoid(`${API}/networks/${encodeURIComponent(name)}/stop`)
export const getNetworkXml = (name: string) =>
  apiGetText(`${API}/networks/${encodeURIComponent(name)}/xml`)

/** Replace persistent network definition. `<name>` in XML must match `name`. Active networks are restarted to apply. */
export const setNetworkXml = (name: string, xml: string) =>
  apiPut<{ status: string; name: string }>(`${API}/networks/${encodeURIComponent(name)}/xml`, { xml })

export const setNetworkAutostart = (name: string, enabled: boolean) => apiPostVoid(`${API}/networks/${encodeURIComponent(name)}/autostart/${enabled}`)
