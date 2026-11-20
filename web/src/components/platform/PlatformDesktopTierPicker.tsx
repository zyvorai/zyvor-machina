// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import {
  PLATFORM_DESKTOP_TIER_HINTS,
  PLATFORM_DESKTOP_TIER_LABELS,
  type PlatformDesktopTier,
} from '../../utils/platformDesktopTier'

export default function PlatformDesktopTierPicker({
  tier,
  onChange,
}: {
  tier: PlatformDesktopTier
  onChange: (t: PlatformDesktopTier) => void
}) {
  const tiers: PlatformDesktopTier[] = ['normal', 'power', 'advanced']

  return (
    <div className="space-y-3">
      <p className="text-sm text-[var(--text-secondary)] leading-relaxed">
        Choose how much of the Machina fleet desktop to show — like macOS simplicity vs. pro tools.
      </p>
      <div className="grid gap-2 sm:grid-cols-3">
        {tiers.map((key) => (
          <button
            key={key}
            type="button"
            onClick={() => onChange(key)}
            className={`rounded-xl border p-3 text-left transition ${
              tier === key ? 'border-[var(--accent)]/50 ring-1 ring-[color-mix(in_srgb,var(--accent)_35%,transparent)] bg-[var(--accent)]/5' : 'border-white/[0.08] hover:border-white/20'
            }`}
          >
            <span className="text-sm font-medium text-[var(--text-primary)] block">{PLATFORM_DESKTOP_TIER_LABELS[key]}</span>
            <span className="text-xs text-[var(--text-muted)] mt-1 block leading-snug">{PLATFORM_DESKTOP_TIER_HINTS[key]}</span>
          </button>
        ))}
      </div>
    </div>
  )
}
