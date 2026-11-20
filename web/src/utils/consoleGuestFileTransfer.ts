// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

type GuestFileTransferPlan = {
  fileName: string
  destPath?: string
  sshUser?: string
  hypervisorHost?: string
  sshNatHostPort?: number | null
  guestIp?: string | null
  guestIpPrivate?: boolean
}

type GuestFileTransferCommand = {
  command: string
  summary: string
}

function shellQuote(value: string): string {
  return `'${value.replace(/'/g, "'\\''")}'`
}

/** Build an scp one-liner to copy a laptop file into the guest (via NAT or direct guest IP). */
export function buildGuestScpCommand(plan: GuestFileTransferPlan): GuestFileTransferCommand | null {
  const fileName = plan.fileName.trim()
  if (!fileName) return null
  // sshUser/hypervisorHost/guestIp can carry attacker-chosen text (e.g. a VM's cloud-init
  // username set by whoever created the VM) that a *different* operator later copies into
  // their own shell from this generated command — quote every segment so it can't smuggle
  // shell metacharacters into that paste, not just the file paths.
  const user = shellQuote(plan.sshUser?.trim() || 'ubuntu')
  const dest = shellQuote(plan.destPath?.trim() || `/tmp/${fileName}`)
  const local = shellQuote(`./${fileName}`)

  if (plan.guestIpPrivate) {
    const host = plan.hypervisorHost?.trim()
    const port = plan.sshNatHostPort
    if (!host || !port || !Number.isFinite(port) || port <= 0) return null
    return {
      command: `scp -P ${Math.trunc(port)} ${local} ${user}@${shellQuote(host)}:${dest}`,
      summary: 'Copy via hypervisor NAT SSH port into the guest',
    }
  }

  const guest = plan.guestIp?.trim()
  if (guest) {
    return {
      command: `scp ${local} ${user}@${shellQuote(guest)}:${dest}`,
      summary: 'Copy directly to the guest IP',
    }
  }

  return null
}
