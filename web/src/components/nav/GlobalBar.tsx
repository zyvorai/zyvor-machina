// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link, useLocation, useNavigate } from 'react-router'
import { ChevronDown, LogOut, Plus, Search, Sparkles } from 'lucide-react'
import { useAuth } from '../../contexts/AuthContext'
import { useAi } from '../../contexts/AiContext'
import { usePlatformInfo } from '../../contexts/PlatformInfoContext'
import { ASK_ZYRA_LABEL } from '../../config/aiBrand'
import { useFleetDesktop } from '../../hooks/useFleetDesktop'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import PlatformControlCenter from '../platform/PlatformControlCenter'
import PlatformMacMenuDropdown, { PlatformMacMenuItem } from '../platform/mac/PlatformMacMenuDropdown'
import { integrationNavItems } from '../../utils/platformIntegrationsNav'
import { menubarProductGroupsForTier, type MenubarProductGroup } from '../../utils/platformMacMenus'
import { navItemActive } from '../../utils/routes'
import { navBlurb } from '../../utils/navBlurbs'
import {
  PLATFORM_DESKTOP_TIER_LABELS,
  type PlatformDesktopTier,
} from '../../utils/platformDesktopTier'
import { dispatchOpenMissionControl } from '../platform/mac/MissionControlContext'
import { CLOSE_PLATFORM_MENUS_EVENT, dispatchOpenSpotlight, OPEN_SPOTLIGHT_EVENT } from '../../utils/platformJarvisShell'
import { dispatchOpenHelp } from '../../utils/openHelp'
import { operationsHubHref } from '../../utils/platformHubLinks'
import { openQuickCreateVm } from '../../hooks/usePlatformVmCreate'

const FLYOUT_CLOSE_DELAY_MS = 650

export default function GlobalBar({ onBurger, needsAttention = 0 }: { onBurger: () => void; needsAttention?: number }) {
  const navigate = useNavigate()
  const location = useLocation()
  const { username, logout } = useAuth()
  const { openCopilot } = useAi()
  const { info } = usePlatformInfo()
  const [tier, setTier] = usePlatformDesktopTier()
    const { desktop } = useFleetDesktop(true, 60_000)

  const [openGroup, setOpenGroup] = useState<string | null>(null)
  // Horizontal centre (px) of the trigger that opened the flyout, so the panel sits under it like Netra's.
  const [flyoutX, setFlyoutX] = useState<number | null>(null)
  const closeTimer = useRef<number | null>(null)

  const groups = useMemo(
    () => menubarProductGroupsForTier(tier, integrationNavItems(info)),
    [tier, info],
  )

  const cancelClose = useCallback(() => {
    if (closeTimer.current !== null) {
      window.clearTimeout(closeTimer.current)
      closeTimer.current = null
    }
  }, [])

  const enter = useCallback((id: string, el?: HTMLElement | null) => {
    cancelClose()
    if (el) { const r = el.getBoundingClientRect(); setFlyoutX(r.left + r.width / 2) }
    setOpenGroup(id)
  }, [cancelClose])

  const scheduleClose = useCallback(() => {
    cancelClose()
    closeTimer.current = window.setTimeout(() => setOpenGroup(null), FLYOUT_CLOSE_DELAY_MS)
  }, [cancelClose])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpenGroup(null)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  useEffect(() => {
    const dismiss = () => setOpenGroup(null)
    window.addEventListener(OPEN_SPOTLIGHT_EVENT, dismiss)
    window.addEventListener(CLOSE_PLATFORM_MENUS_EVENT, dismiss)
    return () => {
      window.removeEventListener(OPEN_SPOTLIGHT_EVENT, dismiss)
      window.removeEventListener(CLOSE_PLATFORM_MENUS_EVENT, dismiss)
    }
  }, [])

  useEffect(() => {
    setOpenGroup(null)
  }, [location.pathname])

  const pickTier = (next: PlatformDesktopTier) => setTier(next)

  const activeGroup = groups.find((g) => g.id === openGroup) ?? null

  return (
    <>
      <header className="gnb-bar" onMouseLeave={scheduleClose}>
        <div className="gnb-bar-inner">
          <button type="button" className="gnb-icon-btn gnb-burger" aria-label="Menu" onClick={onBurger}>
            <span className="gnb-burger-lines" aria-hidden />
          </button>

          <div className="gnb-brand">
            <Link to="/platform" className="gnb-brand-mark" title="Machina" aria-label="Machina home">
              <img src="/zyvor-logomark.svg" width={22} height={22} alt="" className="gnb-logomark" />
            </Link>
            <PlatformMacMenuDropdown
              label="Machina"
              open={openGroup === 'machina'}
              onToggle={() => setOpenGroup((prev) => (prev === 'machina' ? null : 'machina'))}
              onClose={() => setOpenGroup(null)}
            >
              <PlatformMacMenuItem label="About Machina…" onClick={() => { dispatchOpenHelp('about'); setOpenGroup(null) }} />
              {/* Platform guide / Keyboard shortcuts: the old menubar had a separate "Help" menu with
                  these two items plus About; the shell rewrite folded About into this dropdown but
                  dropped the other two, even though the dialogs they open (PlatformAboutHelp.tsx,
                  App.tsx's OPEN_HELP_EVENT listener) are still fully wired — restoring the entry point. */}
              <PlatformMacMenuItem label="Platform guide…" onClick={() => { dispatchOpenHelp('platform'); setOpenGroup(null) }} />
              <PlatformMacMenuItem label="Keyboard shortcuts" onClick={() => { dispatchOpenHelp('shortcuts'); setOpenGroup(null) }} />
              <PlatformMacMenuItem label="Settings…" shortcut="⌘," onClick={() => { navigate('/platform/settings'); setOpenGroup(null) }} />
              <div className="my-1 border-t border-[var(--apple-hairline)]" />
              <PlatformMacMenuItem label="Mission Control" shortcut="F3" onClick={() => { dispatchOpenMissionControl(); setOpenGroup(null) }} />
              <div className="my-1 border-t border-[var(--apple-hairline)]" />
              <PlatformMacMenuItem label={PLATFORM_DESKTOP_TIER_LABELS.normal} checked={tier === 'normal'} onClick={() => pickTier('normal')} />
              <PlatformMacMenuItem label={PLATFORM_DESKTOP_TIER_LABELS.power} checked={tier === 'power'} onClick={() => pickTier('power')} />
              <PlatformMacMenuItem label={PLATFORM_DESKTOP_TIER_LABELS.advanced} checked={tier === 'advanced'} onClick={() => pickTier('advanced')} />
              <div className="my-1 border-t border-[var(--apple-hairline)]" />
              <PlatformMacMenuItem label="Add Host…" onClick={() => { navigate('/platform/enroll'); setOpenGroup(null) }} />
              <div className="my-1 border-t border-[var(--apple-hairline)]" />
              <PlatformMacMenuItem label="Sign Out" onClick={() => { void logout(); setOpenGroup(null) }} />
            </PlatformMacMenuDropdown>
          </div>

          <nav className="gnb-nav" aria-label="Primary">
            <Link
              to="/platform/vms"
              className="gnb-nav-item gnb-nav-link"
              aria-current={location.pathname === '/platform/vms' || location.pathname.startsWith('/platform/hosts/finder') ? 'true' : undefined}
              onMouseEnter={() => setOpenGroup(null)}
            >
              Machine Finder
              {needsAttention > 0 ? <i className="gnb-attn" title={`${needsAttention} need attention`} aria-label={`${needsAttention} need attention`} /> : null}
            </Link>
            {groups.map((group) => (
              <button
                key={group.id}
                type="button"
                className="gnb-nav-item"
                aria-current={group.sections.some((sec) => sec.items.some((item) => item.to !== '/platform/vms' && navItemActive({ to: item.to, label: item.label, icon: null }, location.pathname, location.search))) ? 'true' : undefined}
                aria-expanded={openGroup === group.id}
                onMouseEnter={(e) => enter(group.id, e.currentTarget)}
                onFocus={(e) => enter(group.id, e.currentTarget)}
                // Just open, don't toggle: `onMouseEnter` already opens the flyout as the pointer
                // arrives, so a real click's toggle would immediately close what hover just opened
                // — a mouse user could never click-open a group. Closing already has its own paths
                // (Escape, the scrim, `onMouseLeave` via scheduleClose, route change).
                onClick={(e) => enter(group.id, e.currentTarget)}
              >
                {group.compact}
              </button>
            ))}
          </nav>

          <div className="gnb-right">
            {desktop ? (
              <span className="gnb-live" title={`${desktop.hosts_online}/${desktop.hosts_total} hosts online`}>
                <i />Live
              </span>
            ) : null}
            {desktop && desktop.failed_tasks_24h > 0 ? (
              <Link to={operationsHubHref(tier)} className="gnb-tasks">
                <i />{desktop.failed_tasks_24h} failed task{desktop.failed_tasks_24h === 1 ? '' : 's'}
              </Link>
            ) : null}
            <button type="button" className="gnb-icon-btn" aria-label="Search" title="Search  ⌘K" onClick={() => dispatchOpenSpotlight()}>
              <Search size={17} />
            </button>
            <PlatformControlCenter />
            <button type="button" className="gnb-icon-btn" aria-label={ASK_ZYRA_LABEL} title={ASK_ZYRA_LABEL} onClick={openCopilot}>
              <Sparkles size={17} />
            </button>
            <button type="button" className="gnb-new-vm" onClick={() => openQuickCreateVm()}>
              <Plus size={14} />New VM
            </button>
            <span className="gnb-avatar" title={username || 'user'} aria-hidden>
              {(username || 'U').slice(0, 1).toUpperCase()}
            </span>
            <button type="button" className="gnb-icon-btn" aria-label="Sign out" title="Sign out" onClick={() => void logout()}>
              <LogOut size={15} />
            </button>
          </div>
        </div>

        {activeGroup ? (
          <GlobalBarFlyout
            group={activeGroup}
            anchorX={flyoutX}
            pathname={location.pathname}
            search={location.search}
            onEnter={() => enter(activeGroup.id)}
            onLeave={scheduleClose}
            onNavigate={(to) => { navigate(to); setOpenGroup(null) }}
          />
        ) : null}
      </header>
      {activeGroup ? <div className="gnb-scrim" onClick={() => setOpenGroup(null)} /> : null}
    </>
  )
}

/** Panel width follows the number of columns (Netra: 480-720px) and is clamped to the viewport. */
function flyoutStyle(anchorX: number | null, sections: number): React.CSSProperties {
  const width = Math.min(Math.max(480, sections * 260), 760)
  const vw = typeof window === 'undefined' ? 1440 : window.innerWidth
  const w = Math.min(width, vw - 32)
  const x = anchorX ?? vw / 2
  const left = Math.min(Math.max(16, x - w / 2), vw - w - 16)
  return { width: w, left, right: 'auto' }
}

function GlobalBarFlyout({
  group,
  anchorX,
  pathname,
  search,
  onEnter,
  onLeave,
  onNavigate,
}: {
  group: MenubarProductGroup
  anchorX: number | null
  pathname: string
  search: string
  onEnter: () => void
  onLeave: () => void
  onNavigate: (to: string) => void
}) {
  return (
    <div className="gnb-flyout" onMouseEnter={onEnter} onMouseLeave={onLeave} style={flyoutStyle(anchorX, group.sections.length)}>
      <div className="gnb-flyout-inner">
        <div className="gnb-flyout-sections">
          {group.sections.map((section, i) => (
            <div className="gnb-flyout-section" key={section.label || `${group.id}-${i}`}>
              {section.label ? <p className="gnb-flyout-section-label">{section.label}</p> : null}
              <div className="gnb-flyout-list">
                {section.items.map((item) => {
                  const active = navItemActive({ to: item.to, label: item.label, icon: null }, pathname, search)
                  return (
                    <button
                      key={item.to}
                      type="button"
                      className={`gnb-flyout-link ${active ? 'gnb-flyout-link-active' : ''}`}
                      data-to={item.to}
                      onClick={() => onNavigate(item.to)}
                    >
                      <span className="gnb-flyout-label">{item.label}</span>
                      {navBlurb(item.to) ? <span className="gnb-flyout-blurb">{navBlurb(item.to)}</span> : null}
                    </button>
                  )
                })}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
