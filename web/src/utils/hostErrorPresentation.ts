// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { StructuredPlatformError } from '../components/StructuredErrorBanner'

export function hostErrorPresentation(message: string | null | undefined): StructuredPlatformError | null {
  if (!message?.trim()) return null
  const msg = message
  if (/connection refused|agent unreachable|heartbeat/i.test(msg)) {
    return {
      error_code: 'host_agent_offline',
      message: 'The host agent is not responding — sync and lifecycle actions will fail until it recovers.',
      remediation: 'Verify machina-agent is running on the host, check firewall to the controller, then retry Sync.',
    }
  }
  if (/libvirt|qemu|hypervisor/i.test(msg) && /unavailable|failed|error/i.test(msg)) {
    return {
      error_code: 'libvirt_unreachable',
      message: 'Libvirt on this host is unreachable from the controller.',
      remediation: 'Open classic Node & libvirt tools or Host SSH to verify libvirtd, then run Validate on this host.',
    }
  }
  if (/ssh|authentication|permission denied/i.test(msg)) {
    return {
      error_code: 'host_ssh',
      message: 'SSH enrollment or shell access failed for this host.',
      remediation: 'Check host SSH keys in Settings and use Host SSH from Integrations for a direct terminal.',
    }
  }
  const short = msg.length > 280 ? `${msg.slice(0, 280)}…` : msg
  return {
    error_code: 'host_error',
    message: short,
    remediation: 'Review Tasks for failed host operations, fix the root cause, then retry.',
  }
}
