// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useMemo, useRef, useState } from 'react'
import { Link, NavLink, useLocation } from 'react-router'
import { ChevronDown } from 'lucide-react'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import {
  contextNavForPath,
  isContextNavActive,
  shouldShowContextBar,
  splitContextNavItems,
  type ContextNavItem,
} from '../../utils/platformContextNav'
import PlatformFloatingMenu from '../platform/mac/PlatformFloatingMenu'
import { PlatformMenuLinkItem } from '../platform/mac/PlatformMenuItem'

function ChapterLink({ item, pathname, search }: { item: ContextNavItem; pathname: string; search: string }) {
  const hasQuery = item.to.includes('?')
  const hubExact =
    item.to === '/platform/zeus/security'
    || item.to === '/platform/infrastructure'
    || item.to === '/platform/workloads'
    || item.to === '/platform/administration'
    || item.to === '/platform/operations'
    || item.to === '/platform/settings'
    || item.to === '/platform/zyra'

  if (hasQuery) {
    const active = isContextNavActive(pathname, search, item)
    return (
      <Link
        to={item.to}
        className={`gnb-chapter-link ${active ? 'gnb-chapter-link-active' : ''}`}
        aria-current={active ? 'page' : undefined}
      >
        {item.label}
      </Link>
    )
  }
  return (
    <NavLink
      to={item.to}
      end={hubExact}
      className={({ isActive }) => `gnb-chapter-link ${isActive ? 'gnb-chapter-link-active' : ''}`}
    >
      {item.label}
    </NavLink>
  )
}

export default function ChapterBar() {
  const location = useLocation()
  const [tier] = usePlatformDesktopTier()
  const ctx = contextNavForPath(location.pathname, tier)
  const AppIcon = ctx?.appIcon
  const [moreOpen, setMoreOpen] = useState(false)
  const moreButtonRef = useRef<HTMLButtonElement>(null)

  const { visible, overflow } = useMemo(() => {
    if (!ctx || ctx.items.length <= 1) return { visible: [] as ContextNavItem[], overflow: [] as ContextNavItem[] }
    return splitContextNavItems(ctx.items, location.pathname, location.search)
  }, [ctx, location.pathname, location.search])

  useEffect(() => {
    setMoreOpen(false)
  }, [location.pathname, location.search])

  // The Mission Control home route's own "Mission Control" nav-context label just
  // repeats the SideNav's pinned "Mission Control" link (which is already
  // highlighted as active there) with no extra links to justify the bar — skip it
  // here rather than show the same label twice on screen.
  const isPlatformHome = location.pathname === '/platform' || location.pathname.replace(/\/$/, '') === '/platform'
  if (isPlatformHome || !ctx || !shouldShowContextBar(location.pathname, tier)) return null

  const overflowActive = overflow.some((item) => isContextNavActive(location.pathname, location.search, item))

  return (
    <div className="gnb-chapter" role="navigation" aria-label="Section">
      <div className="gnb-chapter-inner">
        <div className="gnb-chapter-title">
          {AppIcon ? <AppIcon size={16} strokeWidth={1.75} className="gnb-chapter-icon" /> : null}
          {ctx.hubPath ? (
            <Link to={ctx.hubPath} className="gnb-chapter-title-text">{ctx.appLabel}</Link>
          ) : (
            <span className="gnb-chapter-title-text">{ctx.appLabel}</span>
          )}
        </div>

        {ctx.items.length > 1 ? (
          <nav className="gnb-chapter-links" aria-label={`${ctx.appLabel} sections`}>
            {visible.map((item) => (
              <ChapterLink key={item.to} item={item} pathname={location.pathname} search={location.search} />
            ))}
            {overflow.length > 0 ? (
              <div className="relative shrink-0" onClick={(e) => e.stopPropagation()}>
                <button
                  ref={moreButtonRef}
                  type="button"
                  className={`gnb-chapter-link gnb-chapter-more ${overflowActive ? 'gnb-chapter-link-active' : ''}`}
                  aria-expanded={moreOpen}
                  aria-haspopup="menu"
                  onClick={() => setMoreOpen((open) => !open)}
                >
                  More
                  <ChevronDown className={`h-3 w-3 transition-transform ${moreOpen ? 'rotate-180' : ''}`} />
                </button>
                <PlatformFloatingMenu
                  open={moreOpen}
                  onClose={() => setMoreOpen(false)}
                  triggerRef={moreButtonRef}
                  align="start"
                  sideOffset={6}
                  ariaLabel={`${ctx.appLabel} more sections`}
                  className="min-w-[11rem] py-1"
                >
                  {overflow.map((item) => {
                    const active = isContextNavActive(location.pathname, location.search, item)
                    return (
                      <PlatformMenuLinkItem
                        key={item.to}
                        to={item.to}
                        label={item.label}
                        active={active}
                        onNavigate={() => setMoreOpen(false)}
                      />
                    )
                  })}
                </PlatformFloatingMenu>
              </div>
            ) : null}
          </nav>
        ) : (
          <div className="flex-1 min-w-0" />
        )}
      </div>
    </div>
  )
}
