// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { CpuMemoryTopology } from '../api/platformVmLibvirt'
import type { VmGuestHealthReport, VmLibvirtDetails, VmPendingConfig, VmPortForwardRule } from '../api/platform'
import { parseVmGraphicsFromXml } from './vmGraphics'
import { natRuleForGuestPort } from './vmPortForwardServices'

export type VmHardwareSummary = {
  cpu: string
  memory: string
  firmware: string
  tpm: string
  display: string
  video: string
  diskBus: string
  nic: string
  guestAgent: string
  hostDevices: string
  migration: string
  balloonEnabled: boolean
  secureBoot: boolean
  primaryAccess: string
  rdpExposed: boolean
  rdpHostPort: number | null
  isWindows: boolean
}

function extractAttr(block: string, tag: string, attr: string): string | null {
  const re = new RegExp(`<${tag}[^>]*\\b${attr}=['"]([^'"]+)['"]`, 'i')
  const m = block.match(re)
  return m?.[1]?.trim() ?? null
}

function countXmlTag(xml: string, tag: string): number {
  const re = new RegExp(`<${tag}[\\s/>]`, 'g')
  return (xml.match(re) ?? []).length
}

function parseCpuFromXml(xml: string): { mode: string; model: string | null } {
  const cpuBlock = xml.match(/<cpu\b[^>]*>[\s\S]*?<\/cpu>/i)?.[0] ?? xml.match(/<cpu\b[^/>]*\/?>/i)?.[0] ?? ''
  const mode = extractAttr(cpuBlock, 'cpu', 'mode') ?? 'custom'
  const model = extractAttr(cpuBlock, 'model', 'fallback') ?? extractAttr(cpuBlock, 'cpu', 'model')
  return { mode, model }
}

function parseFirmwareFromXml(xml: string): { firmware: string; secureBoot: boolean } {
  const osBlock = xml.match(/<os\b[^>]*>[\s\S]*?<\/os>/i)?.[0] ?? ''
  const loaderBlock = osBlock.match(/<loader\b[^/>]*\/?>/i)?.[0] ?? ''
  const fw = extractAttr(osBlock, 'os', 'firmware') ?? ''
  const loader = loaderBlock.toLowerCase()
  const secure =
    loader.includes("secure='yes'") ||
    loader.includes('secure="yes"') ||
    loader.includes('secure=yes') ||
    fw.toLowerCase().includes('efi') ||
    loader.includes('ovmf') ||
    loader.includes('edk2')
  const isUefi = fw.toLowerCase().includes('efi') || loader.includes('ovmf') || loader.includes('edk2') || secure
  return { firmware: isUefi ? 'UEFI' : 'BIOS', secureBoot: secure && isUefi }
}

function parseTpmFromXml(xml: string): string {
  if (!xml.includes('<tpm')) return 'Not configured'
  const block = xml.match(/<tpm\b[^>]*>[\s\S]*?<\/tpm>/i)?.[0] ?? ''
  const model = extractAttr(block, 'tpm', 'model') ?? 'emulator'
  const persistent = block.includes('persistent') ? ' · persistent' : ''
  if (model.includes('crb') || model.includes('tpm2')) return `TPM 2.0 emulator${persistent}`
  return `TPM ${model}${persistent}`
}

function parseVideoFromXml(xml: string): string {
  const block = xml.match(/<video\b[^>]*\/?>/i)?.[0] ?? ''
  const model = extractAttr(block, 'video', 'model') ?? extractAttr(block, 'model', 'type')
  return model ?? 'default'
}

function parseBalloonFromXml(xml: string): boolean {
  return xml.includes('<memballoon') || xml.includes('<memballoon ')
}

export function buildVmHardwareSummary(input: {
  details: VmLibvirtDetails | null
  domainXml: string
  topology: CpuMemoryTopology | null
  pending: VmPendingConfig | null
  guestHealth: VmGuestHealthReport | null
  portForwardRules: VmPortForwardRule[]
  protocols?: string[]
  osHint?: string
}): VmHardwareSummary {
  const { details, domainXml, topology, pending, guestHealth, portForwardRules, protocols = [], osHint = '' } = input
  const xml = domainXml ?? ''
  const cpuInfo = parseCpuFromXml(xml)
  const fwInfo = parseFirmwareFromXml(xml)
  const graphics = parseVmGraphicsFromXml(xml)
  const displayParts: string[] = []
  if (graphics.spice) displayParts.push('SPICE')
  if (graphics.vnc) displayParts.push('VNC')
  if (protocols.some((p) => p.includes('rdp'))) displayParts.push('RDP')
  if (displayParts.length === 0) displayParts.push('none')

  const vcpus = topology?.vcpus ?? details?.vcpus ?? 0
  const cpuMode = cpuInfo.mode
  const cpuLabel = cpuInfo.model ? `${vcpus} vCPU · ${cpuMode} · ${cpuInfo.model}` : `${vcpus} vCPU · ${cpuMode}`

  const memGiB = topology
    ? Math.round(topology.current_memory_kib / 1024 / 1024)
    : details
      ? Math.round(details.memory_mb / 1024)
      : 0
  const balloon = parseBalloonFromXml(xml)
  const memoryLabel = `${memGiB} GiB${balloon ? ' · balloon enabled' : ''}`

  const fwLabel = fwInfo.secureBoot ? `${fwInfo.firmware} · Secure Boot enabled` : fwInfo.firmware

  const disks = details?.disks.filter((d) => d.device === 'disk') ?? []
  const busSet = [...new Set(disks.map((d) => d.bus || d.driver).filter(Boolean))]
  const diskBus = busSet.length ? busSet.join(', ') : 'virtio'

  const ifaces = details?.interfaces ?? []
  const nicParts = ifaces.slice(0, 2).map((i) => `${i.source || 'network'} · ${i.model || 'virtio'}`)
  const nic = nicParts.length ? nicParts.join('; ') : 'none'

  const agentRunning = guestHealth?.install_state === 'running' && guestHealth.agent_ping
  const guestAgent = agentRunning ? 'running' : guestHealth?.install_state === 'channel_only' ? 'channel only' : 'missing'

  const hostdevCount = countXmlTag(xml, 'hostdev')
  const hostDevices = hostdevCount > 0 ? `${hostdevCount} attached` : 'none'

  let migration = 'safe'
  if (topology?.has_vfio_hostdev) migration = 'warning · VFIO passthrough'
  else if (pending?.needs_shutdown) migration = 'pending restart'

  const rdpRule = natRuleForGuestPort(portForwardRules, 3389)
  const rdpExposed = Boolean(rdpRule)
  const primaryParts = protocols.filter((p) => p !== 'native_ssh' && p !== 'serial')
  const primaryAccess = primaryParts.length ? primaryParts.map((p) => p.replace('_', ' ')).join(' / ') : displayParts.join(' / ')

  const osName = (guestHealth?.os_pretty_name ?? osHint ?? '').toLowerCase()
  const isWindows = osName.includes('windows') || osHint.toLowerCase().includes('windows')

  return {
    cpu: cpuLabel,
    memory: memoryLabel,
    firmware: fwLabel,
    tpm: parseTpmFromXml(xml),
    display: displayParts.join(' / '),
    video: parseVideoFromXml(xml),
    diskBus,
    nic,
    guestAgent,
    hostDevices,
    migration,
    balloonEnabled: balloon,
    secureBoot: fwInfo.secureBoot,
    primaryAccess,
    rdpExposed,
    rdpHostPort: rdpRule?.host_port ?? null,
    isWindows,
  }
}
