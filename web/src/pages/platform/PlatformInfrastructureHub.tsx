// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Database, Network, Map, Server, Archive, Layers, LayoutGrid } from 'lucide-react'
import PlatformPageChrome from '../../components/platform/PlatformPageChrome'
import { AppleDestinationList } from '../../components/platform/apple/AppleStoryKit'

const DESTINATIONS = [
  { to: '/platform/storage', icon: <Database className="w-5 h-5" />, title: 'Storage', subtitle: 'Pools, volumes, and disk tiers' },
  { to: '/platform/networks', icon: <Network className="w-5 h-5" />, title: 'Networks', subtitle: 'Virtual and physical networks' },
  { to: '/platform/network-canvas', icon: <Map className="w-5 h-5" />, title: 'Network Canvas', subtitle: 'Visual topology editor' },
  { to: '/platform/hosts', icon: <Server className="w-5 h-5" />, title: 'Hosts', subtitle: 'Managed hypervisor hosts' },
  { to: '/platform/placement', icon: <Layers className="w-5 h-5" />, title: 'Placement', subtitle: 'Resource scheduling and affinity' },
  { to: '/platform/backups', icon: <Archive className="w-5 h-5" />, title: 'Backups', subtitle: 'Fleet backup schedules and status' },
]

export default function PlatformInfrastructureHub() {
  return (
    <PlatformPageChrome
      eyebrow="Platform"
      title="Infrastructure"
      subtitle="Storage, networking, and physical host resources"
      icon={<LayoutGrid className="w-6 h-6 text-[var(--text-muted)]" />}
    >
      <AppleDestinationList items={DESTINATIONS} />
    </PlatformPageChrome>
  )
}
