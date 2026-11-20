// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonObject } from './client'

const API = '/api/v1'

export interface NodeInfo {
  hostname: string
  hypervisor: string
  hypervisor_version: string
  lib_version: string
  cpu_model: string
  cpu_cores: number
  cpu_threads: number
  cpu_sockets: number
  memory_mb: number
  numa_nodes: number
  active_vms: number
  defined_vms: number
}

export interface HealthStatus {
  status: string
  libvirt: boolean
}

export const getNodeInfo = () => readJsonObject<NodeInfo>(`${API}/node`)
export const getHealth = () => readJsonObject<HealthStatus>(`${API}/health`)
