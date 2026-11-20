// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

export interface GuestAccessHints {
  auth_mode: string
  serial_password_login: boolean
  guest_ip_private: boolean
  ssh_nat_host_port?: number | null
}

/** Windows guests are reached over RDP with a native client, not SSH. */
export function isWindowsGuest(osFamily: string | null | undefined): boolean {
  return (osFamily ?? '').trim().toLowerCase().startsWith('windows')
}

interface AccessHintOpts {
  sshUser?: string
  guestIp?: string
  hypervisorHost?: string
  vmNetworkHref?: string
  /** `os_hint` from the console plan — decides RDP vs SSH guidance. */
  osFamily?: string
  /** Host-side NAT port already forwarded to guest 3389, if any. */
  rdpNatHostPort?: number | null
}

export function consoleAccessHints(
  hints: GuestAccessHints | null | undefined,
  lens: 'serial' | 'shell' | 'display' | string,
  opts: AccessHintOpts,
): string[] {
  if (!hints) return []
  const out: string[] = []
  const user = opts.sshUser?.trim() || 'ubuntu'
  const host = opts.hypervisorHost?.trim() || 'HYPERVISOR_IP'
  const windows = isWindowsGuest(opts.osFamily)

  // Windows: the useful advice is an RDP address for Microsoft Remote Desktop,
  // not an `ssh ubuntu@…` line the guest would refuse anyway.
  if (windows) {
    if (hints.guest_ip_private) {
      if (opts.rdpNatHostPort) {
        out.push(
          `Windows guest ${opts.guestIp ?? ''} is on hypervisor NAT. Connect with Microsoft Remote Desktop (macOS) or mstsc (Windows) to ${host}:${opts.rdpNatHostPort}, or download the .rdp file.`.replace(
            '  ',
            ' ',
          ),
        )
      } else {
        const where = opts.vmNetworkHref ? ` Open ${opts.vmNetworkHref} to expose RDP.` : ' Expose RDP in VM → Network → Hypervisor NAT.'
        out.push(
          `Windows guest ${opts.guestIp ?? '192.168.122.x'} is on hypervisor NAT and is not reachable from your laptop.${where} Then connect with Microsoft Remote Desktop (macOS) or mstsc (Windows).`,
        )
      }
    }
    out.push(
      'Remote Desktop must be enabled inside Windows (System → Remote Desktop). Until it is, use the display console here.',
    )
    return [...new Set(out)]
  }

  const networkLink = opts.vmNetworkHref ? ` Open ${opts.vmNetworkHref} to expose SSH.` : ' Expose SSH in VM → Network → Hypervisor NAT.'

  if (lens === 'serial' && hints.auth_mode === 'ssh_key') {
    out.push(
      `Serial shows a login prompt, but this VM is SSH-key only — no password was configured at create time. Use Shell in ConsoleHub, or expose SSH and connect with your private key.`,
    )
  }

  if ((lens === 'shell' || lens === 'serial') && hints.guest_ip_private) {
    if (hints.ssh_nat_host_port) {
      out.push(
        `Guest IP ${opts.guestIp ?? '192.168.122.x'} is hypervisor NAT only — not reachable from your laptop. Use: ssh -p ${hints.ssh_nat_host_port} ${user}@${host}`,
      )
    } else {
      out.push(
        `Guest IP ${opts.guestIp ?? '192.168.122.x'} is on hypervisor NAT and is not reachable from your laptop.${networkLink} Then: ssh -p 2222 ${user}@${host}`,
      )
    }
  }

  if (lens === 'shell' && hints.auth_mode === 'ssh_key') {
    out.push(
      'In-browser Shell uses the hypervisor’s SSH keys. If login fails, expose SSH and connect from your laptop with the private key that matches the VM’s injected public key.',
    )
  }

  return out
}

/** Short labels for the Cinema Access Note pill (deduped). */
export function aggregateAccessNoteLabels(
  hints: GuestAccessHints | null | undefined,
  opts: AccessHintOpts,
): string[] {
  if (!hints) return []
  const labels: string[] = []
  if (isWindowsGuest(opts.osFamily)) {
    if (opts.rdpNatHostPort) {
      // Lead with the dial-able address: it is the one thing the operator needs
      // to copy into Microsoft Remote Desktop, so it belongs in the collapsed
      // pill rather than behind a disclosure.
      const host = opts.hypervisorHost?.trim() || 'HYPERVISOR_IP'
      labels.push(`RDP ${host}:${opts.rdpNatHostPort}`)
    } else {
      labels.push('Windows guest')
      if (hints.guest_ip_private) labels.push('RDP not exposed')
    }
    if (hints.guest_ip_private) labels.push('NAT guest IP')
    return [...new Set(labels)]
  }
  if (hints.auth_mode === 'ssh_key') labels.push('SSH key-only')
  if (hints.guest_ip_private) labels.push('NAT guest IP')
  if (hints.auth_mode === 'ssh_key' && !hints.serial_password_login) labels.push('Serial has no password')
  if (hints.guest_ip_private && !hints.ssh_nat_host_port) labels.push('SSH not exposed')
  return [...new Set(labels)]
}

export function aggregateAccessNoteMessages(
  hints: GuestAccessHints | null | undefined,
  opts: AccessHintOpts,
): string[] {
  const serial = consoleAccessHints(hints, 'serial', opts)
  const shell = consoleAccessHints(hints, 'shell', opts)
  return [...new Set([...serial, ...shell])]
}
