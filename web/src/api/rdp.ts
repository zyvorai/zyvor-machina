// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonObject } from './client'
import { appendVmConnection } from './vm'

const API = '/api/v1'

interface RdpInfo {
  host: string
  port: number
  ws_path: string
  builtin: boolean
}

export function getRdpInfo(vmName: string, connection?: string | null) {
  const url = appendVmConnection(`${API}/vms/${encodeURIComponent(vmName)}/rdp-info`, connection)
  return readJsonObject<RdpInfo>(url)
}

/** Build a minimal .rdp file for Windows/macOS Remote Desktop clients. */
function buildRdpFile(host: string, port: number, username?: string): string {
  const lines = [
    'screen mode id:i:2',
    'use multimon:i:0',
    'desktopwidth:i:1920',
    'desktopheight:i:1080',
    `full address:s:${host}:${port}`,
    'prompt for credentials:i:1',
    'authentication level:i:2',
    'negotiate security layer:i:1',
  ]
  if (username?.trim()) {
    lines.push(`username:s:${username.trim()}`)
  }
  return lines.join('\r\n') + '\r\n'
}

export function downloadRdpFile(host: string, port: number, vmName: string, username?: string) {
  const blob = new Blob([buildRdpFile(host, port, username)], { type: 'application/rdp' })
  const a = document.createElement('a')
  a.href = URL.createObjectURL(blob)
  a.download = `${vmName}.rdp`
  a.click()
  URL.revokeObjectURL(a.href)
}
