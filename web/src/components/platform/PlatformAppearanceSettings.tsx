// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { MacSettingsGroup, MacSettingsGroupBody } from './mac/PlatformMacUi'
import {
  PLATFORM_WALLPAPER_EVENT,
  PLATFORM_WALLPAPER_LABELS,
  loadPlatformWallpaper,
  resetPlatformWallpaper,
  savePlatformWallpaper,
  type PlatformWallpaper,
} from '../../utils/platformWallpaper'
import { getFleetGeneral, type FleetGeneralOverview } from '../../api/platform'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import PlatformDesktopTierPicker from './PlatformDesktopTierPicker'
import { useTheme, type AppTheme } from '../../contexts/ThemeContext'

const THEME_OPTIONS: { value: AppTheme; label: string; description: string; preview: string }[] = [
  {
    value: 'light',
    label: 'Apple',
    description: 'Default — apple.com white paper (#f5f5f7) with #0071e3 accents',
    preview: 'bg-[#f5f5f7]',
  },
  {
    value: 'dark',
    label: 'Dark',
    description: 'Flat black canvas with #0071e3 accents — matches Netra',
    preview: 'bg-black',
  },
]

const SWATCH_CLASS: Record<PlatformWallpaper, string> = {
  tahoe: 'mac-wallpaper-swatch-tahoe',
  aurora: 'mac-wallpaper-swatch-aurora',
  midnight: 'mac-wallpaper-swatch-midnight',
  ocean: 'mac-wallpaper-swatch-ocean',
}

export default function PlatformAppearanceSettings() {
  const [wallpaper, setWallpaper] = useState<PlatformWallpaper>(() => loadPlatformWallpaper())
  const [general, setGeneral] = useState<FleetGeneralOverview | null>(null)
  const [tier, setTier] = usePlatformDesktopTier()
  const { theme, setTheme } = useTheme()

  useEffect(() => {
    const onChange = () => setWallpaper(loadPlatformWallpaper())
    window.addEventListener(PLATFORM_WALLPAPER_EVENT, onChange)
    return () => window.removeEventListener(PLATFORM_WALLPAPER_EVENT, onChange)
  }, [])

  useEffect(() => {
    void getFleetGeneral().then(setGeneral).catch(() => setGeneral(null))
  }, [])

  const pick = (next: PlatformWallpaper) => {
    savePlatformWallpaper(next)
    setWallpaper(next)
  }

  return (
    <>
      {general && (
        <MacSettingsGroup title="Fleet summary">
          <MacSettingsGroupBody>
            <p className="text-sm text-[var(--text-secondary)] leading-relaxed">{general.summary}</p>
            <div className="grid gap-2 sm:grid-cols-3 text-sm text-[var(--text-muted)]">
              <div>Cluster: <span className="text-[var(--text-primary)]">{general.cluster_name ?? '—'}</span></div>
              <div>Controller: <span className="text-[var(--text-primary)]">{general.controller_version ?? '—'}</span></div>
              <div>VMs: <span className="text-[var(--text-primary)]">{general.vm_count ?? '—'}</span></div>
            </div>
          </MacSettingsGroupBody>
        </MacSettingsGroup>
      )}

      <MacSettingsGroup title="Desktop density">
        <MacSettingsGroupBody>
          <PlatformDesktopTierPicker tier={tier} onChange={setTier} />
          <p className="text-xs text-[var(--text-muted)] leading-relaxed">
            Normal hides the status strip and most menu entries. Advanced restores the full fleet surface.
          </p>
        </MacSettingsGroupBody>
      </MacSettingsGroup>

      <MacSettingsGroup title="Appearance">
        <MacSettingsGroupBody>
          <div>
            <p className="text-sm text-[var(--text-secondary)] leading-relaxed mb-3">Color theme — applies everywhere across the platform.</p>
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
              {THEME_OPTIONS.map((opt) => (
                <button
                  key={opt.value}
                  type="button"
                  onClick={() => setTheme(opt.value)}
                  className={`rounded-xl border p-3 text-left transition ${
                    theme === opt.value
                      ? 'border-[var(--accent)] ring-1 ring-[color-mix(in_srgb,var(--accent)_35%,transparent)]'
                      : 'border-white/[0.08] hover:border-white/20'
                  }`}
                >
                  <div className={`h-12 rounded-lg mb-2 ${opt.preview}`} />
                  <p className="text-xs font-medium text-[var(--text-primary)]">{opt.label}</p>
                  <p className="text-[11px] text-[var(--text-muted)] leading-snug mt-0.5">{opt.description}</p>
                </button>
              ))}
            </div>
          </div>
          <div className="border-t border-white/[0.06] pt-4">
            <p className="text-sm text-[var(--text-secondary)] leading-relaxed mb-3">Desktop wallpaper for the Machina Platform shell (macOS Tahoe style).</p>
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
              {(Object.keys(PLATFORM_WALLPAPER_LABELS) as PlatformWallpaper[]).map((key) => (
                <button
                  key={key}
                  type="button"
                  onClick={() => pick(key)}
                  className={`rounded-xl border p-2 text-left transition ${
                    wallpaper === key
                      ? 'border-[var(--accent)] ring-1 ring-[color-mix(in_srgb,var(--accent)_35%,transparent)]'
                      : 'border-white/[0.08] hover:border-white/20'
                  }`}
                >
                  <div className={`h-16 rounded-lg mb-2 ${SWATCH_CLASS[key]}`} />
                  <span className="text-xs text-[var(--text-primary)]">{PLATFORM_WALLPAPER_LABELS[key]}</span>
                </button>
              ))}
            </div>
            <button type="button" className="btn-secondary text-sm mt-2" onClick={() => { resetPlatformWallpaper(); setWallpaper('tahoe') }}>
              Reset to default
            </button>
          </div>
        </MacSettingsGroupBody>
      </MacSettingsGroup>
    </>
  )
}
