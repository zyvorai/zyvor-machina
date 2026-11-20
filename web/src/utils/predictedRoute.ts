// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** Heuristic default-gateway guess for IPv4 + prefix (common libvirt NAT uses `.1`). */
export function predictedIpv4Gateway(ip: string, prefix: number): string | null {
  if (prefix <= 0 || prefix > 32) return null
  const parts = ip.split('.').map((x) => Number.parseInt(x, 10))
  if (parts.length !== 4 || parts.some((n) => Number.isNaN(n) || n < 0 || n > 255)) return null
  const ipNum = ((parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8) | parts[3]) >>> 0
  const mask = (~0 << (32 - prefix)) >>> 0
  const network = ipNum & mask
  const gw = (network + 1) >>> 0
  return [gw >>> 24, (gw >>> 16) & 255, (gw >>> 8) & 255, gw & 255].join('.')
}
