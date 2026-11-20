// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import { Cpu, GitBranch, Search, Server, Shield, ShieldAlert, Wrench, Workflow, LayoutGrid } from 'lucide-react'
import { MacGlassPanel, LaunchpadAppIcon } from '../mac/PlatformMacUi'
import { ZYRA_HUB_GROUPS, type ZyraHubTile } from '../../../utils/platformZyraHubZones'
import { usePlatformDesktopTier } from '../../../hooks/usePlatformDesktopTier'
import type { PlatformDesktopTier } from '../../../utils/platformDesktopTier'
import { operationsHubHref } from '../../../utils/platformHubLinks'

function tileIcon(tile: ZyraHubTile) {
  switch (tile.id) {
    case 'fleet':
      return <Cpu className="w-8 h-8" strokeWidth={1.75} />
    case 'services':
      return <Workflow className="w-8 h-8" strokeWidth={1.75} />
    case 'baremetal':
      return <Server className="w-8 h-8" strokeWidth={1.75} />
    case 'security-center':
      return <ShieldAlert className="w-8 h-8" strokeWidth={1.75} />
    case 'firewall':
      return <Shield className="w-8 h-8" strokeWidth={1.75} />
    case 'security-tab':
      return <Shield className="w-8 h-8" strokeWidth={1.75} />
    case 'knowledge':
      return <Search className="w-8 h-8" strokeWidth={1.75} />
    case 'mission':
      return <LayoutGrid className="w-8 h-8" strokeWidth={1.75} />
    case 'topology':
      return <GitBranch className="w-8 h-8" strokeWidth={1.75} />
    case 'brain':
      return <GitBranch className="w-8 h-8" strokeWidth={1.75} />
    case 'memory':
      return <Search className="w-8 h-8" strokeWidth={1.75} />
    case 'rightsizing':
      return <Cpu className="w-8 h-8" strokeWidth={1.75} />
    case 'incidents':
      return <ShieldAlert className="w-8 h-8" strokeWidth={1.75} />
    case 'approvals':
      return <Shield className="w-8 h-8" strokeWidth={1.75} />
    case 'operations':
      return <Wrench className="w-8 h-8" strokeWidth={1.75} />
    default:
      return <Cpu className="w-8 h-8" strokeWidth={1.75} />
  }
}

function tileHref(tile: ZyraHubTile, tier: PlatformDesktopTier) {
  if (tile.id === 'operations') return operationsHubHref(tier)
  if (!tile.tab) return tile.to
  if (tile.to === '/platform/zyra') return `${tile.to}?tab=${tile.tab}`
  return tile.to
}

export default function PlatformZyraHubLaunchpad({
  activeTab,
}: {
  activeTab?: string
}) {
  const [tier] = usePlatformDesktopTier()
  return (
    <div className="space-y-5">
      {ZYRA_HUB_GROUPS.map((group) => (
        <MacGlassPanel key={group.label} title={group.label} subtitle={group.subtitle}>
          <div className="platform-launchpad-grid grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 gap-x-4 gap-y-8 -mt-1">
            {group.tiles.map((tile) => {
              const href = tileHref(tile, tier)
              const selected = tile.tab ? activeTab === tile.tab : false
              return (
                <Link key={tile.id} to={href} className="block">
                  <LaunchpadAppIcon
                    name={tile.label}
                    icon={tileIcon(tile)}
                    selected={selected}
                  />
                </Link>
              )
            })}
          </div>
        </MacGlassPanel>
      ))}
    </div>
  )
}
