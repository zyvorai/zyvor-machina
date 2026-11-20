// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import type { CSSProperties } from 'react'
import { Cloud, Server, HardDrive, Plus, Globe, Camera } from 'lucide-react'
import PageLayout from '../components/PageLayout'
import FleetCloudFooter from '../components/FleetCloudFooter'

const DESTINATIONS = [
  {
    to: '/fleet-cloud/instances',
    icon: Server,
    title: 'Instances',
    description: 'Start, stop, console, volumes, and security groups.',
  },
  {
    to: '/fleet-cloud/images',
    icon: HardDrive,
    title: 'Images',
    description: 'Golden images on this hypervisor.',
  },
  {
    to: '/fleet-cloud/create',
    icon: Plus,
    title: 'Create instance',
    description: 'Image, flavor, network, keypair, cloud-init.',
  },
  {
    to: '/fleet-cloud/volumes',
    icon: HardDrive,
    title: 'Volumes',
    description: 'Create, clone, attach, snapshot.',
  },
  {
    to: '/fleet-cloud/flavors',
    icon: Cloud,
    title: 'Flavors',
    description: 'Size catalog for new instances.',
  },
  {
    to: '/fleet-cloud/floating-ips',
    icon: Globe,
    title: 'Floating IPs',
    description: 'Allocate, associate, release.',
  },
  {
    to: '/fleet-cloud/volume-snapshots',
    icon: Camera,
    title: 'Volume snapshots',
    description: 'Snapshot list and restore.',
  },
] as const

const TILE_COLORS = [
  ['#0a84ff', '#5e5ce6'],
  ['#ffd60a', '#ff9f0a'],
  ['#30d158', '#0a84ff'],
  ['#64d2ff', '#0a84ff'],
  ['#bf5af2', '#ff375f'],
  ['#ff9f0a', '#ff375f'],
  ['#5e5ce6', '#64d2ff'],
] as const

export default function FleetCloudOverviewPage() {
  return (
    <PageLayout hideHeader className="!space-y-0 w-full max-w-none" contentClassName="px-0">
      <div className="apple-story-stack w-full">
        <header className="apple-section apple-hero-band nl-aurora-host">
          <span className="nl-aurora" aria-hidden />
          <p className="apple-eyebrow">Fleet Cloud</p>
          <h1 className="apple-display">Native compute</h1>
          <p className="apple-lede">
            Instances and images on this hypervisor — no external cloud required.
          </p>
          <div className="apple-cta-row">
            <Link to="/fleet-cloud/create" className="btn-primary inline-flex items-center gap-2">
              <Plus className="w-4 h-4" />
              Create instance
            </Link>
            <Link to="/fleet-cloud/instances" className="apple-text-link">
              Browse instances <span aria-hidden>›</span>
            </Link>
          </div>
        </header>

        <section className="apple-section">
          <p className="apple-eyebrow">Explore</p>
          <h2 className="apple-display apple-display--sm">What you can do</h2>
          <ul className="nl-tile-grid">
            {DESTINATIONS.map(({ to, title, description, icon: Icon }, i) => {
              const [from, toC] = TILE_COLORS[i % TILE_COLORS.length]
              return (
                <li key={to}>
                  <Link to={to} className="nl-tile" style={{ ['--tile-glow']: from } as CSSProperties}>
                    <span className="nl-tile-icon" style={{ background: `linear-gradient(145deg, ${from}, ${toC})` }} aria-hidden>
                      <Icon className="w-5 h-5" />
                    </span>
                    <span className="min-w-0">
                      <span className="nl-tile-title block">{title}</span>
                      <span className="nl-tile-sub block">{description}</span>
                    </span>
                    <span className="nl-tile-chevron" aria-hidden>›</span>
                  </Link>
                </li>
              )
            })}
          </ul>
        </section>

        <div className="apple-section apple-section--tight">
          <FleetCloudFooter />
        </div>
      </div>
    </PageLayout>
  )
}
