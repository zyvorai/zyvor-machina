// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState } from 'react'
import { Link, useLocation } from 'react-router'
import { ChevronDown } from 'lucide-react'
import { usePlatformInfo } from '../contexts/PlatformInfoContext'
import PlatformFloatingMenu from './platform/mac/PlatformFloatingMenu'
import { PlatformMenuLinkItem } from './platform/mac/PlatformMenuItem'

type Tab = { to: string; label: string; end?: boolean }

/** Primary destinations — kept as pills; everything else under More. */
const PRIMARY: Tab[] = [
  { to: '/fleet-cloud', label: 'Overview', end: true },
  { to: '/fleet-cloud/instances', label: 'Instances' },
  { to: '/fleet-cloud/images', label: 'Images' },
  { to: '/fleet-cloud/volumes', label: 'Volumes' },
  { to: '/fleet-cloud/create', label: 'Create' },
]

const MORE: Tab[] = [
  { to: '/fleet-cloud/volume-snapshots', label: 'Snapshots' },
  { to: '/fleet-cloud/flavors', label: 'Flavors' },
  { to: '/fleet-cloud/qos', label: 'VM QoS' },
  { to: '/fleet-cloud/server-groups', label: 'Groups' },
  { to: '/fleet-cloud/vpcs', label: 'VPCs & elastic compute' },
  { to: '/fleet-cloud/autopilot', label: 'Autopilot' },
  { to: '/fleet-cloud/chaos', label: 'Game days' },
  { to: '/fleet-cloud/preemptible', label: 'Preemptible' },
  { to: '/fleet-cloud/networking', label: 'Network' },
  { to: '/fleet-cloud/topology', label: 'Topology' },
  { to: '/fleet-cloud/floating-ips', label: 'Floating IPs' },
  { to: '/fleet-cloud/heat', label: 'Heat' },
  { to: '/fleet-cloud/load-balancers', label: 'Load balancers' },
  { to: '/fleet-cloud/identity', label: 'Identity' },
  { to: '/fleet-cloud/keypairs', label: 'Keys' },
  { to: '/fleet-cloud/security-groups', label: 'Security' },
]

function tabActive(pathname: string, tab: Tab): boolean {
  if (tab.end) return pathname === tab.to
  return pathname === tab.to || pathname.startsWith(`${tab.to}/`)
}

/** Same language as the top bar and ChapterBar: quiet text, 2px blue underline on the active tab. */
function pillClass(active: boolean): string {
  return `relative inline-flex items-center min-h-11 px-3.5 text-[13px] font-medium tracking-tight transition-colors after:absolute after:left-3.5 after:right-3.5 after:-bottom-px after:h-0.5 after:rounded-full ${
    active
      ? 'text-[var(--text-primary)] after:bg-[var(--apple-link,#0066cc)]'
      : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)] after:bg-transparent'
  }`
}

/** Compact Fleet Cloud section tabs — replaces the old 15-tab strip. */
export default function FleetCloudSubNav() {
  const { pathname } = useLocation()
  const { info } = usePlatformInfo()
  const [moreOpen, setMoreOpen] = useState(false)
  const moreButtonRef = useRef<HTMLButtonElement>(null)
  const moreActive = MORE.some((t) => tabActive(pathname, t))

  useEffect(() => {
    setMoreOpen(false)
  }, [pathname])

  return (
    <div className="mb-6 flex flex-wrap items-center justify-between gap-x-6 border-b border-[var(--apple-hairline)]">
      <nav className="flex flex-wrap items-center" aria-label="Fleet Cloud">
        {PRIMARY.map((tab) => (
          <Link key={tab.to} to={tab.to} className={pillClass(tabActive(pathname, tab))}>
            {tab.label}
          </Link>
        ))}
        <div className="relative">
          <button
            ref={moreButtonRef}
            type="button"
            className={`${pillClass(moreActive)} gap-1`}
            aria-expanded={moreOpen}
            aria-haspopup="menu"
            onClick={() => setMoreOpen((v) => !v)}
          >
            More
            <ChevronDown className={`h-3.5 w-3.5 transition-transform ${moreOpen ? 'rotate-180' : ''}`} />
          </button>
          <PlatformFloatingMenu
            open={moreOpen}
            onClose={() => setMoreOpen(false)}
            triggerRef={moreButtonRef}
            align="start"
            sideOffset={6}
            ariaLabel="Fleet Cloud more destinations"
            className="min-w-[12rem] py-1"
          >
            {MORE.map((tab) => (
              <PlatformMenuLinkItem
                key={tab.to}
                to={tab.to}
                label={tab.label}
                active={tabActive(pathname, tab)}
                onNavigate={() => setMoreOpen(false)}
              />
            ))}
          </PlatformFloatingMenu>
        </div>
      </nav>
      {Boolean(info?.control_plane?.proxy_url) && (
        <div className="flex items-center gap-5 text-[13px]">
          <Link to="/platform" className="apple-link inline-flex items-center min-h-9">
            Platform
          </Link>
          <Link to="/platform/settings?section=integrations" className="apple-link inline-flex items-center min-h-9">
            Integrations
          </Link>
        </div>
      )}
    </div>
  )
}
