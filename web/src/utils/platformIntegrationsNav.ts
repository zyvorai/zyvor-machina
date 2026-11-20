// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React from 'react'
import { Cloud, Cpu, Layers } from 'lucide-react'
import type { PlatformInfo } from '../api/system'
import type { PlatformNavItem } from './platformNav'

const ic = (Icon: React.ComponentType<{ className?: string }>) =>
  React.createElement(Icon, { className: 'w-4 h-4' })

/** External platform links — only routes not already in the sidebar. */
export function integrationNavItems(info: PlatformInfo | null): PlatformNavItem[] {
  if (!info) return []
  const items: PlatformNavItem[] = []

  items.push({ to: '/fleet-cloud', label: 'Fleet Cloud', icon: ic(Cloud) })
  if (info.kubevirt?.exec_enabled) {
    items.push({ to: '/k8s', label: 'Kubernetes', icon: ic(Layers) })
  }
  if (info.fleet?.enabled && (info.fleet.peer_count ?? 0) > 0) {
    items.push({ to: '/fleet', label: 'Multi-site Fleet', icon: ic(Cpu) })
  }

  return items
}

type IntegrationCard = {
  id: string
  title: string
  description: string
  href: string
  enabled: boolean
  configured?: boolean
}

export function integrationCards(info: PlatformInfo | null): IntegrationCard[] {
  if (!info) return []
  return [
    {
      id: 'fleet-cloud',
      title: 'Fleet Cloud',
      description: 'Compute, Network, Storage, Heat, and identity — native, no external cloud required.',
      href: '/fleet-cloud',
      enabled: true,
      configured: true,
    },
    {
      id: 'k8s',
      title: 'Kubernetes',
      description: 'KubeVirt workloads, pods, and cluster exec when enabled on the daemon.',
      href: '/k8s',
      enabled: Boolean(info.kubevirt?.exec_enabled),
    },
    {
      id: 'guestkit',
      title: 'GuestKit',
      description: 'Offline disk doctor, migrate-plan, and inspect job queue.',
      href: '/platform/migration?tab=jobs',
      enabled: Boolean(info.guestkit?.enabled),
    },
    {
      id: 'native-bpf',
      title: 'Native eBPF',
      description: 'machina-bpfd on every host — runtime enforcement, flows, process / DNS telemetry, QoS and packet capture.',
      href: '/platform/zeus/security',
      enabled: true,
      configured: true,
    },
    {
      id: 'classic',
      title: 'Classic Machina UI',
      description: 'Legacy dashboard, VM list, storage pools, and node tools.',
      href: '/',
      enabled: true,
    },
    {
      id: 'fleet',
      title: 'Fleet peers',
      description: 'Multi-controller federation and peer proxy.',
      href: '/fleet',
      enabled: Boolean(info.fleet?.enabled),
      configured: (info.fleet?.peer_count ?? 0) > 0,
    },
  ]
}
