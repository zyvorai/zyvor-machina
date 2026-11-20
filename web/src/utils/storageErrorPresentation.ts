// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { StructuredPlatformError } from '../components/StructuredErrorBanner'

export function storageErrorPresentation(message: string | null | undefined): StructuredPlatformError | null {
  if (!message?.trim()) return null
  const msg = message
  if (/no hosts|empty inventory|0 hosts/i.test(msg)) {
    return {
      error_code: 'no_hosts',
      message: 'No hypervisors are enrolled — storage discovery needs at least one online host.',
      remediation: 'Enroll a host from Platform → Add Host, sync it, then run Import from hosts again.',
    }
  }
  if (/libvirt|pool|storage/i.test(msg) && /not found|failed|error/i.test(msg)) {
    return {
      error_code: 'storage_discover_failed',
      message: 'Libvirt pool discovery failed on one or more hosts.',
      remediation: 'Verify pools exist on the hypervisor (classic Storage or Node tools), sync hosts, then retry import.',
    }
  }
  const short = msg.length > 280 ? `${msg.slice(0, 280)}…` : msg
  return {
    error_code: 'storage_error',
    message: short,
    remediation: 'Check host agent connectivity and libvirt pool paths, then retry Import from hosts.',
  }
}
