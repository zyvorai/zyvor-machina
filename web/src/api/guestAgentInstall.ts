// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { apiGet, apiPost } from './client'

/** Result of injecting the GuestKit agent into a powered-off VM's disk (daemon, same host as the VM). */
export type AgentInjectReport = {
  vm: string
  disk: string
  binary: string
  dry_run: boolean
  output: string
}

/** True when the daemon you are talking to manages this VM (offline injection runs on that host). */
export const daemonManagesVm = (name: string): Promise<boolean> =>
  apiGet<unknown>(`/api/v1/vms/${encodeURIComponent(name)}`).then(() => true, () => false)

/** What GuestKit pieces this hypervisor has. `null` means the daemon is too old to say. */
export type GuestkitCapabilities = {
  cli_found: boolean
  cli_path?: string | null
  agent_binary: string
  agent_binary_found: boolean
}

export const guestkitCapabilities = async (): Promise<GuestkitCapabilities | null> => {
  try {
    const res = await fetch('/api/v1/guest-repair/capabilities', { credentials: 'same-origin' })
    if (!res.ok) return null
    return (await res.json()) as GuestkitCapabilities
  } catch {
    return null
  }
}

/** Checked before anything is shut down so a missing tool never leaves a VM powered off. */
export const injectSupported = async (): Promise<boolean> => {
  const c = await guestkitCapabilities()
  return Boolean(c?.cli_found && c.agent_binary_found)
}

export const injectGuestAgent = (name: string, dryRun = false) =>
  apiPost<AgentInjectReport>(`/api/v1/vms/${encodeURIComponent(name)}/guest-agent/inject`, { dry_run: dryRun })
