// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonArray, readJsonObject, apiPost, apiPostVoid, apiDelete, apiGetText } from './client'

const API = '/api/v1'

export interface CapabilitiesInfo {
  host_arch: string
  host_cpu_model: string
  guests: { os_type: string; arch: string; machines: string[] }[]
  spice_available: boolean
}

export interface NodeDeviceInfo {
  name: string
  parent: string
  driver: string
  capability_type: string
}

export interface NwfilterInfo {
  name: string
  uuid: string
}

export interface SecretInfo {
  uuid: string
  usage_type: string
  usage_id: string
}

interface CreatePoolRequest {
  name: string
  pool_type: string
  target_path: string
}

export const getCapabilities = () => readJsonObject<CapabilitiesInfo>(`${API}/capabilities`)
export const getSysinfo = () => apiGetText(`${API}/sysinfo`)
export const listDevices = () => readJsonArray<NodeDeviceInfo>(`${API}/devices`)
export const getDeviceXml = (name: string) => apiGetText(`${API}/devices/${encodeURIComponent(name)}`)
export const listNwfilters = () => readJsonArray<NwfilterInfo>(`${API}/nwfilters`)
export const getNwfilterXml = (name: string) => apiGetText(`${API}/nwfilters/${encodeURIComponent(name)}`)
export const defineNwfilter = (xml: string) => apiPost<{ status: string; name: string }>(`${API}/nwfilters`, { xml })
export const deleteNwfilter = (name: string) => apiDelete(`${API}/nwfilters/${encodeURIComponent(name)}`)
export const listSecrets = () => readJsonArray<SecretInfo>(`${API}/secrets`)

interface DefineSecretRequest {
  xml: string
  value_base64?: string
  validate_xml?: boolean
  set_value_flags?: number
}

export const defineSecret = (req: DefineSecretRequest) =>
  apiPost<{ status: string; uuid: string }>(`${API}/secrets`, req)

export const deleteSecret = (uuid: string) => apiDelete(`${API}/secrets/${encodeURIComponent(uuid)}`)
export const getSecretXml = (uuid: string) => apiGetText(`${API}/secrets/${encodeURIComponent(uuid)}`)
export const createPool = (req: CreatePoolRequest) => apiPost<unknown>(`${API}/storage/pools`, req)
export const deletePool = (name: string) => apiDelete(`${API}/storage/pools/${encodeURIComponent(name)}`)
export const getPoolXml = (name: string) => apiGetText(`${API}/storage/pools/${encodeURIComponent(name)}/xml`)
export const resizeVolume = (pool: string, vol: string, capacityGb: number) => apiPostVoid(`${API}/storage/pools/${encodeURIComponent(pool)}/volumes/${encodeURIComponent(vol)}/resize`, { capacity_gb: capacityGb })
export const cloneVolume = (pool: string, vol: string, newName: string) => apiPostVoid(`${API}/storage/pools/${encodeURIComponent(pool)}/volumes/${encodeURIComponent(vol)}/clone`, { new_name: newName })

export const attachPciHostdev = (vmName: string, pci: string) =>
  apiPostVoid(`${API}/vms/${encodeURIComponent(vmName)}/hostdev/pci/attach`, { pci })

export const detachPciHostdev = (vmName: string, pci: string) =>
  apiPostVoid(`${API}/vms/${encodeURIComponent(vmName)}/hostdev/pci/detach`, { pci })

export const detachNodeDevice = (devName: string) =>
  apiPostVoid(`${API}/host/nodedev/${encodeURIComponent(devName)}/detach`)

export const reattachNodeDevice = (devName: string) =>
  apiPostVoid(`${API}/host/nodedev/${encodeURIComponent(devName)}/reattach`)

export interface VmJobStats {
  cur: number
  end: number
  bandwidth: number
  job_type?: string
}

export const getVmJobStats = (vmName: string) =>
  readJsonObject<VmJobStats>(`${API}/vms/${encodeURIComponent(vmName)}/job/stats`)

export interface CpuCompareResult {
  /** True when the host can run this guest CPU definition (identical or host is a superset). */
  compatible: boolean
  summary: string
  /** Raw libvirt compare label: identical | superset | incompatible | unknown */
  label: string
  code: number
}

type CpuCompareApiResult = {
  code: number
  label: string
}

/** Compare a guest `<cpu>…</cpu>` XML fragment against this host (`virConnectCompareCPU`). */
export async function compareCpu(body: { cpu_xml: string; flags?: number }): Promise<CpuCompareResult> {
  const raw = await apiPost<CpuCompareApiResult>(`${API}/cpu/compare`, {
    cpu_xml: body.cpu_xml,
    flags: body.flags ?? 0,
  })
  const label = raw.label || 'unknown'
  const compatible = label === 'identical' || label === 'superset'
  const summary =
    label === 'identical'
      ? 'Guest CPU definition is identical to this host.'
      : label === 'superset'
        ? 'Host CPU is a superset of the guest definition (safe to run here).'
        : label === 'incompatible'
          ? 'Guest CPU is incompatible with this host.'
          : `CPU compare result: ${label} (code ${raw.code}).`
  return { compatible, summary, label, code: raw.code }
}

export const getLocalFirewallInventory = () =>
  readJsonObject<Record<string, unknown>>(`${API}/zeus-firewall/local/inventory`)
