// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { createVmPortForward } from '../api/platform'

export type PortForwardAccessKind = 'ssh' | 'http' | 'https' | 'tcp'

export interface PortForwardServiceTemplate {
  id: string
  name: string
  vmPort: number
  hostPort: number
  access: PortForwardAccessKind
  /** Built-in catalog entry vs user-saved custom template. */
  builtin?: boolean
}

export const KNOWN_PORT_FORWARD_SERVICES: PortForwardServiceTemplate[] = [
  { id: 'ssh', name: 'SSH', vmPort: 22, hostPort: 2222, access: 'ssh', builtin: true },
  { id: 'http', name: 'HTTP', vmPort: 80, hostPort: 9080, access: 'http', builtin: true },
  { id: 'https', name: 'HTTPS', vmPort: 443, hostPort: 9443, access: 'https', builtin: true },
  { id: 'http-alt', name: 'HTTP (8080)', vmPort: 8080, hostPort: 18080, access: 'http', builtin: true },
  { id: 'https-alt', name: 'HTTPS (8443)', vmPort: 8443, hostPort: 18443, access: 'https', builtin: true },
  { id: 'mysql', name: 'MySQL', vmPort: 3306, hostPort: 13306, access: 'tcp', builtin: true },
  { id: 'postgres', name: 'PostgreSQL', vmPort: 5432, hostPort: 15432, access: 'tcp', builtin: true },
  { id: 'redis', name: 'Redis', vmPort: 6379, hostPort: 16379, access: 'tcp', builtin: true },
  { id: 'mongodb', name: 'MongoDB', vmPort: 27017, hostPort: 37017, access: 'tcp', builtin: true },
  { id: 'rdp', name: 'RDP', vmPort: 3389, hostPort: 13389, access: 'tcp', builtin: true },
  { id: 'vnc', name: 'VNC', vmPort: 5900, hostPort: 15900, access: 'tcp', builtin: true },
  { id: 'grafana', name: 'Grafana', vmPort: 3000, hostPort: 13000, access: 'http', builtin: true },
  { id: 'prometheus', name: 'Prometheus', vmPort: 9090, hostPort: 19090, access: 'http', builtin: true },
  { id: 'elasticsearch', name: 'Elasticsearch', vmPort: 9200, hostPort: 19200, access: 'http', builtin: true },
  { id: 'k8s-api', name: 'Kubernetes API', vmPort: 6443, hostPort: 16443, access: 'tcp', builtin: true },
]

const CUSTOM_STORAGE_PREFIX = 'machina-port-forward-custom:'

function customServicesStorageKey(platformVmId: string): string {
  return `${CUSTOM_STORAGE_PREFIX}${platformVmId}`
}

export function loadCustomPortForwardServices(platformVmId: string): PortForwardServiceTemplate[] {
  if (typeof window === 'undefined' || !platformVmId) return []
  try {
    const raw = window.localStorage.getItem(customServicesStorageKey(platformVmId))
    if (!raw) return []
    const parsed = JSON.parse(raw) as PortForwardServiceTemplate[]
    return Array.isArray(parsed)
      ? parsed.filter((s) => s?.name && s.vmPort > 0 && s.hostPort > 0)
      : []
  } catch {
    return []
  }
}

export function saveCustomPortForwardService(
  platformVmId: string,
  service: PortForwardServiceTemplate,
): PortForwardServiceTemplate[] {
  const existing = loadCustomPortForwardServices(platformVmId)
  const next = [...existing.filter((s) => s.id !== service.id), service]
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(customServicesStorageKey(platformVmId), JSON.stringify(next))
  }
  return next
}

export function deleteCustomPortForwardService(platformVmId: string, id: string): PortForwardServiceTemplate[] {
  const next = loadCustomPortForwardServices(platformVmId).filter((s) => s.id !== id)
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(customServicesStorageKey(platformVmId), JSON.stringify(next))
  }
  return next
}

export function suggestHostPort(guestPort: number, taken: number[] = []): number {
  const known = KNOWN_PORT_FORWARD_SERVICES.find((s) => s.vmPort === guestPort)
  if (known && !taken.includes(known.hostPort)) return known.hostPort
  const candidates = [
    guestPort >= 1024 && guestPort <= 49151 ? guestPort : null,
    9000 + guestPort,
    10000 + guestPort,
    20000 + guestPort,
  ].filter((p): p is number => p != null && p <= 65535)
  return candidates.find((p) => !taken.includes(p)) ?? Math.min(65535, guestPort + 40000)
}

export function publicHostname(explicit?: string): string {
  const raw = explicit?.trim()
  if (raw && isSafePublicHostname(raw)) return raw
  return typeof window !== 'undefined' ? window.location.hostname : ''
}

/** Hostnames/IPs safe to embed in ssh/http URLs (no path, scheme, or whitespace injection). */
function isSafePublicHostname(value: string): boolean {
  if (!value || value.includes('..')) return false
  return /^[a-zA-Z0-9.\-[\]:]+$/.test(value)
}

export function isPrivateGuestIp(ip: string): boolean {
  const parts = ip.trim().split('.').map((p) => parseInt(p, 10))
  if (parts.length !== 4 || parts.some((n) => !Number.isFinite(n) || n < 0 || n > 255)) return false
  const [a, b] = parts
  if (a === 10) return true
  if (a === 172 && b >= 16 && b <= 31) return true
  if (a === 192 && b === 168) return true
  if (a === 169 && b === 254) return true
  return false
}

export type NatRuleLike = { protocol: string; host_port: number; vm_port: number }

export function natRuleForGuestPort(rules: NatRuleLike[], guestPort: number): NatRuleLike | undefined {
  return rules.find((r) => r.protocol === 'tcp' && r.vm_port === guestPort)
}

export function sshNatHostPort(rules: NatRuleLike[]): number | undefined {
  return natRuleForGuestPort(rules, 22)?.host_port
}

function templateForGuestPort(guestPort: number): PortForwardServiceTemplate | undefined {
  return KNOWN_PORT_FORWARD_SERVICES.find((s) => s.vmPort === guestPort)
}

function buildExposePayload(
  vmName: string,
  guestPort: number,
  takenHostPorts: number[] = [],
  customName?: string,
): { host_port: number; vm_port: number; protocol: 'tcp'; description: string } {
  const known = templateForGuestPort(guestPort)
  const hostPort = known && !takenHostPorts.includes(known.hostPort)
    ? known.hostPort
    : suggestHostPort(guestPort, takenHostPorts)
  const label = customName || known?.name || `TCP ${guestPort}`
  return {
    protocol: 'tcp',
    host_port: hostPort,
    vm_port: guestPort,
    description: `${vmName}:${label}`,
  }
}

export function takenHostPorts(rules: Array<{ host_port: number }>): number[] {
  return rules.map((r) => r.host_port)
}

/** Create a NAT port-forward rule for a guest TCP port (SSH, RDP, etc.). */
export async function exposeGuestPortOnVm(
  vmId: string,
  vmName: string,
  guestPort: number,
  existingRules: Array<{ host_port: number }>,
  customName?: string,
) {
  return createVmPortForward(
    vmId,
    buildExposePayload(vmName, guestPort, takenHostPorts(existingRules), customName),
  )
}

export function laptopSshCommand(
  sshUser: string,
  guestIp: string,
  hypervisorHost: string | undefined,
  rules: NatRuleLike[],
  fallbackNatPort = 2222,
): string {
  const user = sshUser.trim() || 'ubuntu'
  const host = publicHostname(hypervisorHost)
  if (guestIp && isPrivateGuestIp(guestIp)) {
    const natPort = sshNatHostPort(rules) ?? fallbackNatPort
    return `ssh -p ${natPort} ${user}@${host || 'HYPERVISOR_IP'}`
  }
  if (!guestIp.trim()) return ''
  return `ssh ${user}@${guestIp.trim()}`
}

export function laptopHttpHref(
  guestPort: number,
  hypervisorHost: string | undefined,
  rules: NatRuleLike[],
  guestIp: string,
): string | undefined {
  if (!HTTP_ACCESS_PORTS.has(guestPort)) return undefined
  const host = publicHostname(hypervisorHost)
  if (!host) return undefined
  if (guestIp && isPrivateGuestIp(guestIp)) {
    const rule = natRuleForGuestPort(rules, guestPort)
    const hostPort = rule?.host_port ?? suggestHostPort(guestPort)
    const scheme = guestPort === 443 || guestPort === 8443 ? 'https' : 'http'
    return `${scheme}://${host}:${hostPort}/`
  }
  if (!guestIp.trim()) return undefined
  const scheme = guestPort === 443 || guestPort === 8443 ? 'https' : 'http'
  return `${scheme}://${guestIp.trim()}:${guestPort}/`
}

const HTTP_ACCESS_PORTS = new Set([80, 443, 8080, 8443, 8000, 3000, 9090, 9200])

export function serviceAccessLabel(
  service: Pick<PortForwardServiceTemplate, 'access' | 'hostPort'>,
  sshUser = 'ubuntu',
  hostname?: string,
): string {
  const host = publicHostname(hostname)
  switch (service.access) {
    case 'ssh':
      return `ssh -p ${service.hostPort} ${sshUser}@${host || 'HOST'}`
    case 'http':
      return `http://${host || 'HOST'}:${service.hostPort}/`
    case 'https':
      return `https://${host || 'HOST'}:${service.hostPort}/`
    default:
      return `${host || 'HOST'}:${service.hostPort}`
  }
}

export function serviceAccessHref(
  service: Pick<PortForwardServiceTemplate, 'access' | 'hostPort'>,
  hostname?: string,
): string | undefined {
  const host = publicHostname(hostname)
  if (!host) return undefined
  if (service.access === 'http') return `http://${host}:${service.hostPort}/`
  if (service.access === 'https') return `https://${host}:${service.hostPort}/`
  return undefined
}

export function ruleMatchesService(
  rule: { host_port: number; vm_port: number; protocol: string },
  service: Pick<PortForwardServiceTemplate, 'hostPort' | 'vmPort'>,
): boolean {
  return rule.protocol === 'tcp' && rule.host_port === service.hostPort && rule.vm_port === service.vmPort
}

export function resolveServiceForRule(
  rule: { host_port: number; vm_port: number; protocol?: string; description?: string },
  catalog: PortForwardServiceTemplate[],
): PortForwardServiceTemplate | null {
  const normalized = { ...rule, protocol: rule.protocol ?? 'tcp' }
  const match = catalog.find((s) => ruleMatchesService(normalized, s))
  if (match) return match
  const desc = rule.description?.trim()
  if (desc) {
    return {
      id: `rule-${rule.host_port}-${rule.vm_port}`,
      name: desc,
      vmPort: rule.vm_port,
      hostPort: rule.host_port,
      access: inferAccessKind(rule.vm_port),
    }
  }
  return {
    id: `rule-${rule.host_port}-${rule.vm_port}`,
    name: `TCP ${rule.vm_port}`,
    vmPort: rule.vm_port,
    hostPort: rule.host_port,
    access: inferAccessKind(rule.vm_port),
  }
}

function inferAccessKind(vmPort: number): PortForwardAccessKind {
  if (vmPort === 22) return 'ssh'
  if (vmPort === 443 || vmPort === 8443) return 'https'
  if (vmPort === 80 || vmPort === 8080 || vmPort === 3000 || vmPort === 9090 || vmPort === 9200) return 'http'
  return 'tcp'
}

export function newCustomServiceId(name: string): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 32)
  return `custom-${slug || 'service'}-${Date.now().toString(36)}`
}

export function inferAccessFromPorts(vmPort: number, preferHttp = false): PortForwardAccessKind {
  if (preferHttp && (vmPort === 80 || vmPort === 8080 || vmPort === 3000)) return 'http'
  return inferAccessKind(vmPort)
}
