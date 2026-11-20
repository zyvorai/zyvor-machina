// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { StructuredPlatformError } from '../components/StructuredErrorBanner'

export function vmErrorPresentation(lastError: string | null | undefined): StructuredPlatformError | null {
  if (!lastError?.trim()) return null
  const msg = lastError
  // Stale no-op power failures — VM is already in the desired state.
  if (/domain is already running|already running/i.test(msg) && /resume|start/i.test(msg)) {
    return null
  }
  if (/nodomain|domain not found|no domain with matching name/i.test(msg)) {
    return {
      error_code: 'domain_missing',
      message: 'This VM no longer exists on the hypervisor (libvirt domain missing). The inventory record is stale.',
      remediation: 'Delete this VM from the platform, or run host sync after recreating the domain on KVM.',
    }
  }
  if (/connection refused|transport error/i.test(msg)) {
    return {
      error_code: 'agent_unreachable',
      message: 'The host agent is not reachable — lifecycle actions cannot run until the agent is online.',
      remediation: 'Check machina-agent on the host and verify the host shows online in Hosts.',
    }
  }
  const short = msg.length > 280 ? `${msg.slice(0, 280)}…` : msg
  return {
    error_code: 'vm_error',
    message: short,
    remediation: 'Open Tasks for the failed operation, fix the root cause, then retry.',
  }
}
