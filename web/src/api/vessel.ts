// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { apiDelete, apiGet, apiPost } from './client'

type EngineKind = 'podman' | 'docker' | 'Podman' | 'Docker'

interface EngineCapabilities {
  pods: boolean
  logs: boolean
  stats: boolean
}

export interface VesselStatus {
  enabled: boolean
  connected: boolean
  socket?: string | null
  error?: string | null
  engine?: EngineKind
  version?: string
  name?: string
  os?: string
  arch?: string
  cpus?: number
  memory_total?: number
  capabilities?: EngineCapabilities
}

interface PortMapping {
  ip?: string | null
  private_port: number
  public_port?: number | null
  typ: string
}

export interface ContainerSummary {
  id: string
  name: string
  image: string
  status: string
  state: string
  created: string
  ports: PortMapping[]
  labels: Record<string, string>
}

export interface PodSummary {
  id: string
  name: string
  status: string
  infra_container_id?: string | null
  containers: string[]
  created: string
  labels: Record<string, string>
}

function shortId(id: string): string {
  return id.length > 12 ? id.slice(0, 12) : id
}

export { shortId }

export function getVesselStatus(): Promise<VesselStatus> {
  return apiGet<VesselStatus>('/api/v1/vessel/status')
}

export function reconnectVessel(): Promise<VesselStatus> {
  return apiPost<VesselStatus>('/api/v1/vessel/reconnect')
}

export async function listVesselContainers(all = true): Promise<ContainerSummary[]> {
  const data = await apiGet<{ items: ContainerSummary[] }>(
    `/api/v1/vessel/containers?all=${all ? 'true' : 'false'}`,
  )
  return data.items ?? []
}

export function createVesselContainer(body: {
  name: string
  image: string
  command?: string[]
  start?: boolean
}): Promise<{ id: string; name: string }> {
  return apiPost('/api/v1/vessel/containers', {
    name: body.name,
    image: body.image,
    command: body.command ?? [],
    start: body.start ?? true,
  })
}

export function runVesselWindowsDockur(body: {
  guest: 'win10' | 'win11' | 'windows-server-2022' | 'windows-server-2025'
  name?: string
  use_golden?: boolean
}): Promise<{
  id: string
  name: string
  guest: string
  engine: string
  golden: boolean
  web: string
  rdp: string
  login: string
  message: string
}> {
  return apiPost('/api/v1/vessel/windows-dockur', body)
}

export function startVesselContainer(id: string): Promise<{ status: string; name: string }> {
  return apiPost(`/api/v1/vessel/containers/${encodeURIComponent(id)}/start`)
}

export function stopVesselContainer(id: string): Promise<{ status: string; name: string }> {
  return apiPost(`/api/v1/vessel/containers/${encodeURIComponent(id)}/stop`)
}

export function restartVesselContainer(id: string): Promise<{ status: string; name: string }> {
  return apiPost(`/api/v1/vessel/containers/${encodeURIComponent(id)}/restart`)
}

export function removeVesselContainer(id: string, force = false): Promise<void> {
  return apiDelete(`/api/v1/vessel/containers/${encodeURIComponent(id)}?force=${force ? 'true' : 'false'}`)
}

export async function listVesselPods(): Promise<PodSummary[]> {
  const data = await apiGet<{ items: PodSummary[] }>('/api/v1/vessel/pods')
  return data.items ?? []
}

export function createVesselPod(name: string, labels: Record<string, string> = {}): Promise<{ id: string; name: string }> {
  return apiPost('/api/v1/vessel/pods', { name, labels })
}

export function startVesselPod(id: string): Promise<{ status: string; name: string }> {
  return apiPost(`/api/v1/vessel/pods/${encodeURIComponent(id)}/start`)
}

export function stopVesselPod(id: string): Promise<{ status: string; name: string }> {
  return apiPost(`/api/v1/vessel/pods/${encodeURIComponent(id)}/stop`)
}

export function removeVesselPod(id: string, force = false): Promise<void> {
  return apiDelete(`/api/v1/vessel/pods/${encodeURIComponent(id)}?force=${force ? 'true' : 'false'}`)
}
