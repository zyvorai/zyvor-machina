// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'
import { Link } from 'react-router'
import { Plus } from 'lucide-react'
import { GlassCard } from '../../glass/GlassCard'
import { GlassModal } from '../../glass/GlassModal'
import { navActiveChipClasses, statusBgClass } from '../../../utils/semanticColors'

export function MacGlassPanel({
  title,
  subtitle,
  action,
  children,
  className = '',
  'data-testid': dataTestId,
}: {
  title?: string
  subtitle?: string
  action?: React.ReactNode
  children: React.ReactNode
  className?: string
  'data-testid'?: string
}) {
  return (
    <GlassCard hover={false} className={`platform-mac-panel tahoe-glass-card p-0 ${className}`} data-testid={dataTestId}>
      {(title || action) && (
        <header className="flex items-start justify-between gap-3 px-4 pt-3.5 pb-2 border-b border-white/[0.04]">
          <div>
            {title && <h2 className="font-semibold text-[var(--text-primary)] text-sm">{title}</h2>}
            {subtitle && <p className="text-xs text-[var(--text-muted)] mt-0.5">{subtitle}</p>}
          </div>
          {action}
        </header>
      )}
      <div className="p-4">{children}</div>
    </GlassCard>
  )
}

export function MacSectionTitle({ title, subtitle }: { title: string; subtitle?: string }) {
  return (
    <div className="mb-6 platform-readable">
      <h2 className="section-title text-[var(--text-primary)]">{title}</h2>
      {subtitle && <p className="page-lede !mt-2 !text-[1.0625rem]">{subtitle}</p>}
    </div>
  )
}

export function MacSheet({
  open,
  onClose,
  title,
  subtitle,
  children,
  wide,
  ariaLabel,
}: {
  open: boolean
  onClose: () => void
  title: string
  subtitle?: string
  children: React.ReactNode
  wide?: boolean
  ariaLabel?: string
}) {
  return (
    <GlassModal open={open} onClose={onClose} title={title} subtitle={subtitle} wide={wide} ariaLabel={ariaLabel}>
      <div className="-mx-1 px-1">{children}</div>
    </GlassModal>
  )
}

// Apple app-icon-style palette — bold, distinct hues (blue/green/purple/orange/
// pink/teal/yellow/indigo) instead of the near-black grayscale rotation this used
// to be, so app tiles read as distinct at a glance the way real Apple icons do.
const GRADIENTS = [
  'from-[#0A84FF] to-[#0060DF]',
  'from-[#30D158] to-[#248A3D]',
  'from-[#BF5AF2] to-[#8944AB]',
  'from-[#FF9F0A] to-[#D97706]',
  'from-[#FF375F] to-[#D70015]',
  'from-[#64D2FF] to-[#0091FF]',
  'from-[#FFD60A] to-[#D4A100]',
  'from-[#5E5CE6] to-[#3634A3]',
] as const

export function gradientForName(name: string): string {
  let h = 0
  for (let i = 0; i < name.length; i += 1) h = (h + name.charCodeAt(i) * 17) % GRADIENTS.length
  return GRADIENTS[h]!
}

export function LaunchpadAppIcon({
  name,
  icon,
  vmCount,
  gradient,
  selected,
  onClick,
}: {
  name: string
  icon: React.ReactNode
  vmCount?: number
  gradient?: string
  selected?: boolean
  onClick?: () => void
}) {
  const g = gradient ?? gradientForName(name)
  // Only the hardcoded-dark GRADIENTS set is theme-independent — a caller-supplied
  // `gradient` can be a light theme-token background (e.g. quick-create presets), where
  // the light-theme text-white override correctly darkens the icon instead. Force a
  // pure-white icon (bypassing that override) only when `g` is actually one of the dark set.
  const isDarkGradient = (GRADIENTS as readonly string[]).includes(g)
  const Tag = onClick ? 'button' : 'div'
  return (
    <Tag
      type={onClick ? 'button' : undefined}
      onClick={onClick}
      className={`platform-launchpad-icon group flex flex-col items-center gap-2.5 text-center w-full ${onClick ? 'cursor-pointer' : ''}`}
    >
      <div
        className={`relative w-[72px] h-[72px] sm:w-[84px] sm:h-[84px] rounded-[22%] bg-gradient-to-br ${g} shadow-lg shadow-black/30 flex items-center justify-center ${isDarkGradient ? 'text-[#ffffff]' : 'text-white'} transition-transform group-hover:scale-105 group-active:scale-95 ${
          selected ? 'ring-2 ring-white/80 ring-offset-2 ring-offset-slate-950' : ''
        }`}
      >
        {icon}
        {vmCount != null && vmCount > 0 && (
          <span className="absolute -top-1 -right-1 min-w-[1.25rem] h-5 px-1 rounded-full bg-[var(--apple-surface)]/90 border border-white/20 text-[10px] font-semibold flex items-center justify-center">
            {vmCount}
          </span>
        )}
      </div>
      <span className="text-xs sm:text-sm font-medium text-[var(--text-primary)] max-w-[7rem] leading-tight line-clamp-2">{name}</span>
    </Tag>
  )
}

export function NewLaunchpadCard({ onClick, label = 'New Application', subtitle = 'Create from VMs' }: { onClick: () => void; label?: string; subtitle?: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="platform-launchpad-icon group flex flex-col items-center gap-2.5 text-center w-full"
    >
      <div className="w-[72px] h-[72px] sm:w-[84px] sm:h-[84px] rounded-[22%] border-2 border-dashed border-[var(--apple-hairline)]/80 bg-[var(--apple-surface)] flex items-center justify-center text-[var(--text-muted)] group-hover:border-[var(--accent)]/60 group-hover:text-[var(--accent)] group-hover:bg-[var(--accent-soft)] transition-all group-hover:scale-105 group-active:scale-95">
        <Plus className="w-8 h-8" />
      </div>
      <span className="text-xs sm:text-sm font-medium text-[var(--text-secondary)] max-w-[7rem] leading-tight">{label}</span>
      <span className="text-xs text-[var(--text-muted)] -mt-1">{subtitle}</span>
    </button>
  )
}

export function PresetTemplateCard({
  name,
  description,
  icon,
  gradient,
  onClick,
}: {
  name: string
  description: string
  icon: React.ReactNode
  gradient: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex items-center gap-3 p-3 rounded-xl border border-white/[0.06] bg-[var(--apple-surface)] hover:bg-[var(--apple-fill-tertiary)]/70 hover:border-white/10 transition text-left w-full"
    >
      <div className={`w-11 h-11 shrink-0 rounded-xl bg-gradient-to-br ${gradient} flex items-center justify-center text-white shadow-md`}>
        {icon}
      </div>
      <div className="min-w-0">
        <p className="text-sm font-medium text-[var(--text-primary)] truncate">{name}</p>
        <p className="text-xs text-[var(--text-muted)] truncate">{description}</p>
      </div>
    </button>
  )
}

/** macOS Settings-style toggle switch */
export function MacToggle({
  checked,
  onChange,
  disabled,
  label,
  description,
  id,
}: {
  checked: boolean
  onChange: (next: boolean) => void
  disabled?: boolean
  label: string
  description?: string
  id?: string
}) {
  const toggleId = id ?? label.replace(/\s+/g, '-').toLowerCase()
  return (
    <div className="flex items-start justify-between gap-4 px-4 py-3 border-b border-white/[0.04] last:border-0">
      <div className="min-w-0">
        <label htmlFor={toggleId} className="text-sm font-medium text-[var(--text-primary)] cursor-pointer">
          {label}
        </label>
        {description && <p className="text-xs text-[var(--text-muted)] mt-0.5 leading-relaxed">{description}</p>}
      </div>
      <button
        id={toggleId}
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`platform-mac-toggle relative shrink-0 w-11 h-6 rounded-full transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--machina-status-info)] ${
          checked ? statusBgClass('ok') : 'bg-[var(--apple-fill-secondary)]'
        } ${disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}`}
      >
        <span
          className={`absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white shadow transition-transform ${
            checked ? 'translate-x-5' : 'translate-x-0'
          }`}
        />
      </button>
    </div>
  )
}

/** Segmented control (Stealth mode, etc.) */
export function MacSegmentedControl<T extends string>({
  options,
  value,
  onChange,
  label,
}: {
  options: Array<{ value: T; label: string; icon?: ReactNode }>
  value: T
  onChange: (v: T) => void
  label?: string
}) {
  return (
    <div className="space-y-2">
      {label && <p className="text-xs font-medium text-[var(--text-muted)] uppercase tracking-wide">{label}</p>}
      <div className="inline-flex p-0.5 rounded-full bg-black/30 border border-white/[0.08] tahoe-segment" role="tablist">
        {options.map((opt) => (
          <button
            key={opt.value}
            type="button"
            role="tab"
            aria-selected={value === opt.value}
            onClick={() => onChange(opt.value)}
            className={`px-3 py-1.5 min-h-[34px] text-xs font-medium rounded-full transition flex items-center gap-1.5 ${
              value === opt.value
                ? 'tahoe-segment-active text-[var(--text-primary)]'
                : 'text-[var(--text-muted)] hover:text-[var(--text-primary)]'
            }`}
          >
            {opt.icon}
            {opt.label}
          </button>
        ))}
      </div>
    </div>
  )
}

/** Settings list row (Security & Privacy style) */
export function MacListRow({
  title,
  subtitle,
  trailing,
  href,
  onClick,
  badge,
}: {
  title: string
  subtitle?: string
  trailing?: React.ReactNode
  href?: string
  onClick?: () => void
  badge?: React.ReactNode
}) {
  const cls =
    'flex items-center gap-3 px-4 py-3 border-b border-white/[0.04] last:border-0 hover:bg-white/[0.02] transition text-left w-full'
  const inner = (
    <>
      <div className="flex-1 min-w-0">
        <p className="text-sm text-[var(--text-primary)] truncate">{title}</p>
        {subtitle && <p className="text-xs text-[var(--text-muted)] truncate mt-0.5">{subtitle}</p>}
      </div>
      {badge}
      {trailing}
    </>
  )
  if (href) {
    return (
      <Link to={href} className={cls}>
        {inner}
      </Link>
    )
  }
  if (onClick) {
    return (
      <button type="button" onClick={onClick} className={cls}>
        {inner}
      </button>
    )
  }
  return <div className={cls}>{inner}</div>
}

/** Settings shell: sticky grouped section nav on the left (desktop), scrollable chips on top (small screens). */
export function MacSettingsPane({
  sections,
  active,
  onSelect,
  title,
  hideTitle = false,
  children,
}: {
  sections: Array<{ id: string; label: string; icon?: React.ReactNode; group?: string }>
  active: string
  onSelect: (id: string) => void
  title: string
  /** Keep the heading for assistive tech but not on screen (the page chrome already shows it). */
  hideTitle?: boolean
  children: React.ReactNode
}) {
  const groups: Array<{ name: string; items: typeof sections }> = []
  for (const s of sections) {
    const name = s.group ?? ''
    const g = groups.find((x) => x.name === name)
    if (g) g.items.push(s)
    else groups.push({ name, items: [s] })
  }
  const btn = (s: (typeof sections)[number], chip: boolean) => (
    <button
      key={s.id}
      type="button"
      onClick={() => onSelect(s.id)}
      aria-current={active === s.id ? 'page' : undefined}
      className={
        chip
          ? `inline-flex shrink-0 items-center gap-2 px-3.5 py-2 rounded-full text-sm transition ${
              active === s.id
                ? navActiveChipClasses()
                : 'text-[var(--text-muted)] bg-[var(--apple-fill-tertiary)]/50 hover:bg-[var(--apple-fill-tertiary)] hover:text-[var(--text-primary)]'
            }`
          : `flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-sm transition ${
              active === s.id
                ? 'bg-[color-mix(in_srgb,var(--apple-link,#0071e3)_12%,transparent)] font-medium text-[var(--apple-link,#0071e3)]'
                : 'text-[var(--text-secondary)] hover:bg-[var(--apple-fill-tertiary)] hover:text-[var(--text-primary)]'
            }`
      }
    >
      <span className="shrink-0 opacity-90">{s.icon}</span>
      {s.label}
    </button>
  )
  return (
    <div className="w-full max-w-none">
      <h2 className={hideTitle ? 'sr-only' : 'apple-display text-2xl sm:text-3xl text-[var(--text-primary)] tracking-tight mb-3'}>{title}</h2>
      <div className="grid gap-6 lg:grid-cols-[14.5rem_minmax(0,1fr)] lg:items-start">
        {/* Phone/tablet: one scrollable row of chips */}
        <nav className="-mx-1 flex gap-2 overflow-x-auto px-1 pb-1 lg:hidden [scrollbar-width:none]" aria-label={`${title} sections`}>
          {sections.map((s) => btn(s, true))}
        </nav>
        {/* Desktop: sticky grouped rail */}
        <nav className="hidden lg:block lg:sticky lg:top-16 lg:max-h-[calc(100dvh-5rem)] lg:overflow-y-auto space-y-5 pr-1" aria-label={`${title} sections`}>
          {groups.map((g) => (
            <div key={g.name || 'all'}>
              {g.name ? <p className="px-3 pb-1.5 text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--text-muted)]">{g.name}</p> : null}
              <div className="space-y-0.5">{g.items.map((s) => btn(s, false))}</div>
            </div>
          ))}
        </nav>
        <div className="mac-settings-detail w-full min-w-0 platform-readable text-[var(--text-primary)]">
          {children}
        </div>
      </div>
    </div>
  )
}

export function MacSettingsGroup({
  title,
  children,
}: {
  title?: string
  children: React.ReactNode
}) {
  return (
    <section className="mb-6 last:mb-0">
      {title && (
        <h3 className="text-sm font-medium text-[var(--text-secondary)] mb-2 px-1">{title}</h3>
      )}
      <div className="rounded-xl border border-white/[0.08] bg-[var(--apple-surface)] overflow-hidden divide-y divide-white/[0.05] text-[var(--text-secondary)]">
        {children}
      </div>
    </section>
  )
}

/** Padded prose block inside a MacSettingsGroup (list rows stay full-bleed). */
export function MacSettingsGroupBody({
  children,
  className = '',
}: {
  children: React.ReactNode
  className?: string
}) {
  return <div className={`px-4 py-3 space-y-3 ${className}`}>{children}</div>
}
