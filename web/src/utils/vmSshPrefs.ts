// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** Remember last SSH target for a VM (browser sessionStorage). */

const storageKey = (vmName: string) => `machina:vm-ssh:${encodeURIComponent(vmName)}`

interface VmSshPrefs {
  host: string
  user: string
}

export function loadVmSshPrefs(vmName: string): VmSshPrefs | null {
  try {
    const raw = sessionStorage.getItem(storageKey(vmName))
    if (!raw) return null
    const o = JSON.parse(raw) as { host?: unknown; user?: unknown }
    const host = typeof o.host === 'string' ? o.host.trim() : ''
    const user = typeof o.user === 'string' ? o.user.trim() : ''
    if (!host && !user) return null
    return { host, user: user || 'root' }
  } catch {
    return null
  }
}

export function saveVmSshPrefs(vmName: string, prefs: VmSshPrefs): void {
  try {
    sessionStorage.setItem(
      storageKey(vmName),
      JSON.stringify({ host: prefs.host.trim(), user: prefs.user.trim() || 'root' }),
    )
  } catch {
    /* quota / private mode */
  }
}
