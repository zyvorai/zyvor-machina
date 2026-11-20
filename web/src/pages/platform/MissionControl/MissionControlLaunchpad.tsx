// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import { Camera, Layers, Monitor, Network, Plus, ShieldCheck, Tv } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'

type Props = {
  onCreateVm: () => void
}

const DESTINATIONS: Array<{
  id: string
  label: string
  subtitle: string
  href?: string
  action?: 'create'
  icon: LucideIcon
  from: string
  to: string
}> = [
  { id: 'finder', label: 'Machine Finder', subtitle: 'Inventory, power, migrate', href: '/platform/vms', icon: Monitor, from: '#0a84ff', to: '#5e5ce6' },
  { id: 'create', label: 'Create a VM', subtitle: 'Four steps from image to boot', action: 'create', icon: Plus, from: '#30d158', to: '#0a84ff' },
  { id: 'live-wall', label: 'Live Preview Wall', subtitle: 'Live VNC thumbnails of running VMs', href: '/platform/mission-control/live', icon: Tv, from: '#bf5af2', to: '#ff375f' },
  { id: 'security', label: 'Security', subtitle: 'Firewall, risk, compliance', href: '/platform/zeus/security', icon: ShieldCheck, from: '#ff9f0a', to: '#ff375f' },
  { id: 'recovery', label: 'Recovery', subtitle: 'Snapshots and backups', href: '/platform/backups', icon: Camera, from: '#64d2ff', to: '#0a84ff' },
  { id: 'templates', label: 'Templates', subtitle: 'Golden images', href: '/platform/templates', icon: Layers, from: '#ffd60a', to: '#ff9f0a' },
  { id: 'network', label: 'Network', subtitle: 'Path and topology', href: '/platform/network-canvas', icon: Network, from: '#5e5ce6', to: '#64d2ff' },
]

export default function MissionControlLaunchpad({ onCreateVm }: Props) {
  return (
    <section className="apple-section" data-testid="mission-control-launchpad">
      <p className="apple-eyebrow">Explore</p>
      <h2 className="apple-display apple-display--sm">Operations</h2>
      <p className="apple-lede">A few places that matter. Everything else is one search away.</p>

      <ul className="nl-tile-grid">
        {DESTINATIONS.map((d) => {
          const Icon = d.icon
          const body = (
            <>
              <span className="nl-tile-icon" style={{ background: `linear-gradient(145deg, ${d.from}, ${d.to})` }} aria-hidden>
                <Icon className="w-5 h-5" />
              </span>
              <span className="min-w-0">
                <span className="nl-tile-title block">{d.label}</span>
                <span className="nl-tile-sub block">{d.subtitle}</span>
              </span>
              <span className="nl-tile-chevron" aria-hidden>›</span>
            </>
          )
          return (
            <li key={d.id}>
              {d.action === 'create' ? (
                <button type="button" className="nl-tile" style={{ ['--tile-glow' as string]: d.from }} onClick={onCreateVm}>
                  {body}
                </button>
              ) : (
                <Link to={d.href!} className="nl-tile" style={{ ['--tile-glow' as string]: d.from }}>
                  {body}
                </Link>
              )}
            </li>
          )
        })}
      </ul>
    </section>
  )
}
