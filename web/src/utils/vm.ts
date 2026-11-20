// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { vmStatusBadgeClasses } from './vmVisual'

export function getStateBadgeClasses(state: string): string {
  return vmStatusBadgeClasses(state)
}

export function formatBytes(bytes: number): string {
  // Binary divisors (1024) with binary unit labels — the values are byte counts
  // from libvirt/cgroups, which are power-of-two. Previously divided by 1024 but
  // labeled GB/MB/KB (decimal), so a 500 GB disk read "465.7 GB" — wrong number
  // for the symbol. Matches the GiB formatters used elsewhere in the codebase.
  if (!Number.isFinite(bytes)) return '—'
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(1)} GiB`
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)} MiB`
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KiB`
  return `${Math.round(bytes)} B`
}

/** Bytes per second for throughput / I/O rates from cumulative counters. */
export function formatThroughput(bytesPerSec: number): string {
  return `${formatBytes(bytesPerSec)}/s`
}
