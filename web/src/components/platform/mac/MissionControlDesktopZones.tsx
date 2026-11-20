// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { cloneElement, isValidElement, useMemo, type ReactNode } from 'react'
import { Link, useLocation } from 'react-router'
import { sidebarForTier } from '../../../utils/platformNavFilter'
import { integrationNavItems } from '../../../utils/platformIntegrationsNav'
import { usePlatformInfo } from '../../../contexts/PlatformInfoContext'
import { usePlatformDesktopTier } from '../../../hooks/usePlatformDesktopTier'
import { loadPlatformDesktopTabs } from '../../../utils/platformDesktopTabs'
import { LaunchpadAppIcon } from './PlatformMacUi'

function scaleIcon(icon: ReactNode) {
  if (isValidElement<{ className?: string; strokeWidth?: number }>(icon)) {
    return cloneElement(icon, { className: 'w-8 h-8', strokeWidth: 1.75 })
  }
  return icon
}

function sectionSubtitle(label: string): string | undefined {
  switch (label) {
    case 'Favorites':
      return 'Pinned destinations'
    case 'Fleet':
      return 'Zyra OS and integrations'
    case 'Platform':
      return 'Resources, operations, and security'
    case 'Connected platforms':
      return 'External clouds and fleet peers'
    default:
      return undefined
  }
}

export default function MissionControlDesktopZones({ onNavigate }: { onNavigate: () => void }) {
  const [tier] = usePlatformDesktopTier()
  const { info } = usePlatformInfo()
  const location = useLocation()
  const sections = useMemo(
    () => sidebarForTier(tier, integrationNavItems(info)),
    [tier, info],
  )
  const tabs = loadPlatformDesktopTabs()

  return (
    <div className="mission-control-desktop space-y-5">
      {tabs.length > 1 && (
        <div className="rounded-2xl border border-white/[0.06] bg-[var(--apple-surface)] p-4 space-y-3">
          <h3 className="text-sm font-semibold text-[var(--text-primary)]">Open windows</h3>
          <div className="flex gap-3 overflow-x-auto pb-1 -mx-1 px-1">
            {tabs.map((tab) => {
              const active = tab.path === location.pathname
              return (
                <Link
                  key={tab.path}
                  to={tab.path}
                  onClick={onNavigate}
                  className={`mission-control-window-card shrink-0 min-w-[9rem] max-w-[12rem] rounded-xl border px-3 py-2.5 transition ${
                    active
                      ? 'border-[var(--accent)]/50 bg-[var(--accent)]/10 text-[var(--text-primary)]'
                      : 'border-white/[0.08] bg-[var(--apple-surface)] text-[var(--text-primary)] hover:border-white/15 hover:bg-[var(--apple-surface)]'
                  }`}
                >
                  <span className="block text-sm font-medium truncate">{tab.label}</span>
                  <span className="block text-[10px] text-[var(--text-muted)] truncate mt-0.5">{tab.path.replace('/platform', '') || '/'}</span>
                </Link>
              )
            })}
          </div>
        </div>
      )}

      <div className="grid gap-5 xl:grid-cols-2">
        {sections.map((section) => (
          <div key={section.label} className="rounded-2xl border border-white/[0.06] bg-[var(--apple-surface)] p-4 space-y-4">
            <div>
              <h3 className="text-sm font-semibold text-[var(--text-primary)]">{section.label}</h3>
              {sectionSubtitle(section.label) && (
                <p className="text-xs text-[var(--text-muted)] mt-0.5">{sectionSubtitle(section.label)}</p>
              )}
            </div>
            <div className="platform-launchpad-grid grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 gap-x-4 gap-y-8 -mt-1">
              {section.items.map((item) => {
                const active =
                  location.pathname === item.to ||
                  (item.to !== '/platform' && location.pathname.startsWith(`${item.to}/`))
                return (
                  <Link key={item.to} to={item.to} onClick={onNavigate} className="block">
                    <LaunchpadAppIcon
                      name={item.label}
                      icon={scaleIcon(item.icon)}
                      selected={active}
                    />
                  </Link>
                )
              })}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
