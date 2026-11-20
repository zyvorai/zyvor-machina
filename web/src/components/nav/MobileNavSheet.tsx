// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useMemo, useRef } from 'react'
import { Link, NavLink, useLocation, useNavigate } from 'react-router'
import { ChevronDown, Plus, Search, Settings, X } from 'lucide-react'
import { usePlatformInfo } from '../../contexts/PlatformInfoContext'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import { integrationNavItems } from '../../utils/platformIntegrationsNav'
import { menubarProductGroupsForTier } from '../../utils/platformMacMenus'
import { dispatchOpenSpotlight } from '../../utils/platformJarvisShell'
import { navItemActive } from '../../utils/routes'
import { openQuickCreateVm } from '../../hooks/usePlatformVmCreate'

/**
 * Full-height navigation sheet for phones and tablets (<= 1024px), where the top-bar group buttons
 * are hidden. It renders the same groups as the desktop flyouts as collapsible sections; the group
 * that contains the current page starts open.
 */
export default function MobileNavSheet({
  open,
  onClose,
  needsAttention = 0,
}: {
  open: boolean
  onClose: () => void
  needsAttention?: number
}) {
  const location = useLocation()
  const navigate = useNavigate()
  const { info } = usePlatformInfo()
  const [tier] = usePlatformDesktopTier()
  const groups = useMemo(() => menubarProductGroupsForTier(tier, integrationNavItems(info)), [tier, info])
  const panelRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    document.addEventListener('keydown', onKey)
    const prev = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    panelRef.current?.focus()
    return () => { document.removeEventListener('keydown', onKey); document.body.style.overflow = prev }
  }, [open, onClose])

  // Close after navigating.
  const firstPath = useRef(location.pathname + location.search)
  useEffect(() => {
    if (firstPath.current === location.pathname + location.search) return
    firstPath.current = location.pathname + location.search
    onClose()
  }, [location.pathname, location.search, onClose])

  if (!open) return null

  const isActive = (to: string) => navItemActive({ to, label: '', icon: null }, location.pathname, location.search)

  return (
    <div className="gnb-sheet" role="dialog" aria-modal="true" aria-label="Navigation" ref={panelRef} tabIndex={-1}>
      <div className="gnb-sheet-head">
        <Link to="/platform" className="gnb-sheet-brand" onClick={onClose}>
          <img src="/zyvor-logomark.svg" width={22} height={22} alt="" className="gnb-logomark" />
          <span>Machina</span>
        </Link>
        <button type="button" className="gnb-icon-btn" aria-label="Close menu" onClick={onClose}>
          <X size={18} />
        </button>
      </div>

      <nav className="gnb-sheet-body" aria-label="Primary">
        <NavLink to="/platform" end className="gnb-sheet-link">Mission Control</NavLink>
        <NavLink to="/platform/vms" className="gnb-sheet-link">
          Machine Finder
          {needsAttention > 0 ? <span className="gnb-sheet-count">{needsAttention}</span> : null}
        </NavLink>

        {groups.map((group) => {
          const hasActive = group.sections.some((s) => s.items.some((i) => isActive(i.to)))
          return (
            <details key={group.id} className="gnb-sheet-group" open={hasActive}>
              <summary>
                <span>{group.compact}</span>
                <ChevronDown size={16} aria-hidden />
              </summary>
              {group.sections.map((section, idx) => (
                <div key={section.label || `${group.id}-${idx}`} className="gnb-sheet-section">
                  {section.label ? <p className="gnb-sheet-section-label">{section.label}</p> : null}
                  {section.items.map((item) => (
                    <NavLink
                      key={item.to}
                      to={item.to}
                      className={`gnb-sheet-link gnb-sheet-link--item${isActive(item.to) ? ' is-active' : ''}`}
                    >
                      {item.label}
                    </NavLink>
                  ))}
                </div>
              ))}
            </details>
          )
        })}
      </nav>

      <div className="gnb-sheet-foot">
        <button type="button" className="btn-secondary text-sm" onClick={() => { onClose(); dispatchOpenSpotlight() }}>
          <Search size={15} /> Search
        </button>
        <button type="button" className="btn-secondary text-sm" onClick={() => { onClose(); navigate('/platform/settings') }}>
          <Settings size={15} /> Settings
        </button>
        <button type="button" className="btn-primary text-sm" onClick={() => { onClose(); openQuickCreateVm() }}>
          <Plus size={15} /> New VM
        </button>
      </div>
    </div>
  )
}
