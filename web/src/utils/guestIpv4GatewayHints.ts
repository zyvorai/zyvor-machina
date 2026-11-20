// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { GuestIpAddress, VmDetails } from '../api/vm'
import { predictedIpv4Gateway } from './predictedRoute'

/** Libvirt network XML gateway vs .1 subnet heuristic for a guest address row. */
export function guestIpv4GatewayHints(
  ip: GuestIpAddress,
  vm: VmDetails | null | undefined,
  networkGateways: Record<string, string> | undefined,
): { xmlGateway: string | null; heuristicGateway: string | null } {
  const heuristicGateway =
    ip.ip_type === 'ipv4' ? predictedIpv4Gateway(ip.address, ip.prefix) : null
  const mac = ip.mac?.toLowerCase()
  const iface = vm?.interfaces?.find((i) => i.mac_address.toLowerCase() === mac)
  const net = iface?.source
  const g = networkGateways ?? {}
  const xmlGateway = net && g[net] ? g[net] : null
  return { xmlGateway, heuristicGateway }
}
